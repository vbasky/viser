#!/usr/bin/env python3
"""
Train an ExtraTrees regressor for viser's R-D point prediction and export to ONNX.

Usage:
  1. Collect per-title analysis JSON outputs from real encodes:
     viser per-title analyze -i video1.mp4 -o video1.json ...
     viser per-title analyze -i video2.mp4 -o video2.json ...

  2. Run this script:
     python3 train_extra_trees.py --data *.json -o predictor.onnx

The model maps 7 features (complexity, spatial, temporal, log_pixels,
codec_efficiency, crf_norm, audio_bitrate_kbps) to 2 outputs (log_bitrate, vmaf).

Requires: sklearn, onnx, onnxmltools, numpy
  pip install scikit-learn onnx onnxmltools numpy
"""

import argparse
import json
import glob
import math
import sys

import numpy as np
from sklearn.ensemble import ExtraTreesRegressor
from sklearn.model_selection import train_test_split
from sklearn.metrics import r2_score, mean_absolute_error

# ── Feature extraction ──────────────────────────────────────────────────────

CODEC_EFF = {
    "libx264": 1.0, "h264_nvenc": 1.0, "h264_qsv": 1.0,
    "h264_videotoolbox": 1.0, "h264_vaapi": 1.0, "h264_amf": 1.0,
    "libx265": 0.72, "hevc_nvenc": 0.72, "hevc_qsv": 0.72,
    "hevc_videotoolbox": 0.72, "hevc_vaapi": 0.72, "hevc_amf": 0.72,
    "libsvtav1": 0.58, "av1_nvenc": 0.58, "av1_qsv": 0.58,
    "av1_vaapi": 0.58, "av1_amf": 0.58,
    "libvpx-vp9": 0.65,
}


def extract_features(points, complexity, audio_bitrate_kbps):
    """Extract feature vectors and targets from per-title analysis data.

    Args:
        points: List of viser Point dicts with resolution, codec, crf, bitrate, vmaf
        complexity: viser Complexity dict with overall_score, avg_spatial, avg_temporal
        audio_bitrate_kbps: Audio bitrate overhead

    Yields:
        (features_7d, log_bitrate, vmaf) tuples
    """
    c = complexity or {"overall_score": 50.0, "avg_spatial": 0.7, "avg_temporal": 15.0}
    audio_kbps = audio_bitrate_kbps or 0.0

    for pt in points:
        pixels = pt["resolution"]["width"] * pt["resolution"]["height"]
        codec_eff = CODEC_EFF.get(pt["codec"], 0.7)
        crf_norm = pt["crf"] / 63.0

        features = [
            c["overall_score"] / 100.0,          # 0: normalised complexity
            c["avg_spatial"],                      # 1: spatial detail
            min(c["avg_temporal"] / 75.0, 1.0),   # 2: normalised motion
            math.log2(max(pixels, 1)),             # 3: log resolution
            codec_eff,                             # 4: codec efficiency
            crf_norm,                              # 5: normalised CRF
            audio_kbps,                            # 6: audio overhead
        ]

        log_bitrate = math.log(max(pt["bitrate"], 1.0))
        vmaf = pt["vmaf"]

        yield features, log_bitrate, vmaf


def load_json(path):
    """Load a viser per-title analysis JSON file."""
    with open(path) as f:
        return json.load(f)


# ── Training ────────────────────────────────────────────────────────────────

def train(data_paths, output_path):
    X_bitrate, y_log_bitrate = [], []
    X_vmaf, y_vmaf = [], []

    for pattern in data_paths:
        for path in sorted(glob.glob(pattern)):
            print(f"  Loading {path} ...")
            analysis = load_json(path)
            complexity = analysis.get("complexity")
            audio_kbps = 0.0
            if "source_info" in analysis:
                audio = analysis["source_info"].get("audio_stream") or {}
                audio_kbps = audio.get("bit_rate", 0) / 1000.0

            for feats, log_br, vmaf in extract_features(
                analysis.get("points", []), complexity, audio_kbps
            ):
                X_bitrate.append(feats)
                y_log_bitrate.append(log_br)
                X_vmaf.append(feats)
                y_vmaf.append(vmaf)

    if not X_bitrate:
        print("ERROR: no training data found. Check --data paths.")
        sys.exit(1)

    X_b = np.array(X_bitrate, dtype=np.float32)
    X_v = np.array(X_vmaf, dtype=np.float32)
    y_b = np.array(y_log_bitrate, dtype=np.float32)
    y_v = np.array(y_vmaf, dtype=np.float32)

    print(f"\nTraining samples: {len(X_b)}")
    print(f"  Bitrate range: {np.exp(y_b).min():.0f} – {np.exp(y_b).max():.0f} kbps")
    print(f"  VMAF range:    {y_v.min():.1f} – {y_v.max():.1f}")

    # ── Train bitrate regressor ──
    print("\nTraining bitrate model...")
    br_model = ExtraTreesRegressor(
        n_estimators=200, max_depth=16, min_samples_leaf=4,
        random_state=42, n_jobs=-1,
    )
    X_b_train, X_b_test, y_b_train, y_b_test = train_test_split(
        X_b, y_b, test_size=0.15, random_state=42
    )
    br_model.fit(X_b_train, y_b_train)
    y_b_pred = br_model.predict(X_b_test)
    print(f"  Bitrate R²:  {r2_score(y_b_test, y_b_pred):.4f}")
    print(f"  Bitrate MAE: {mean_absolute_error(y_b_test, y_b_pred):.4f} (log)")

    # ── Train VMAF regressor ──
    print("\nTraining VMAF model...")
    vmaf_model = ExtraTreesRegressor(
        n_estimators=200, max_depth=16, min_samples_leaf=4,
        random_state=42, n_jobs=-1,
    )
    X_v_train, X_v_test, y_v_train, y_v_test = train_test_split(
        X_v, y_v, test_size=0.15, random_state=42
    )
    vmaf_model.fit(X_v_train, y_v_train)
    y_v_pred = vmaf_model.predict(X_v_test)
    print(f"  VMAF R²:     {r2_score(y_v_test, y_v_pred):.4f}")
    print(f"  VMAF MAE:    {mean_absolute_error(y_v_test, y_v_pred):.4f}")

    # ── Export to ONNX ──
    print(f"\nExporting to {output_path} ...")
    export_to_onnx(br_model, vmaf_model, output_path, X_b.shape[1])
    print("Done.")


# ── ONNX export ─────────────────────────────────────────────────────────────

def export_to_onnx(br_model, vmaf_model, output_path, n_features):
    try:
        from skl2onnx import convert_sklearn
        from skl2onnx.common.data_types import FloatTensorType
    except ImportError:
        print("Install skl2onnx via: pip install skl2onnx")
        sys.exit(1)

    # Wrap both models as a single pipeline: concatenate outputs
    initial_types = [("input", FloatTensorType([None, n_features]))]
    options = {id(br_model): {"zipmap": False}, id(vmaf_model): {"zipmap": False}}

    onnx_model = convert_sklearn(
        (br_model, vmaf_model),
        initial_types=initial_types,
        options=options,
    )

    with open(output_path, "wb") as f:
        f.write(onnx_model.SerializeToString())


# ── CLI ─────────────────────────────────────────────────────────────────────

if __name__ == "__main__":
    parser = argparse.ArgumentParser(
        description="Train ExtraTrees R-D point predictor for viser"
    )
    parser.add_argument(
        "--data", nargs="+", required=True,
        help="Glob pattern(s) for per-title analysis JSON files",
    )
    parser.add_argument(
        "-o", "--output", default="predictor.onnx",
        help="Output ONNX model path (default: predictor.onnx)",
    )
    args = parser.parse_args()
    train(args.data, args.output)
