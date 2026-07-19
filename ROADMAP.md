# viser roadmap

viser today is a **content-adaptive VOD encoding toolkit**: probe, analyze,
encode, and measure quality — all from a single Rust binary. This document
tracks what's covered, what's missing, and what's planned.

Priorities are ordered by impact; checkboxes track status. Nothing here is a
commitment to a date.

## Status snapshot

**Covered:** per-title convex-hull analysis, per-shot Trellis bit allocation,
segment-level CRF tuning, content-adaptive encoding profiles, shot detection
(FFmpeg scdet), CRF / capped-CRF / fixed-QP / two-pass VBR encoding,
checkpoint/resume, audio-bitrate-aware ladder budgets, screen-content
detection with automatic encoding-strategy adjustment, an optional pure-Rust
probe engine (`revelo`), a broad quality-metric suite (VMAF, PSNR, SSIM,
MS-SSIM, VIF, XPSNR, CAMBI, SSIMULACRA2, butteraugli + no-reference signals
including NIQE/BRISQUE) with metric-vs-metric comparison, faithfulness scoring
(v0), the comparison player, hardware encoder support (NVENC, QuickSync,
VideoToolbox, VAAPI, AMF — H.264/H.265/AV1), HLS/DASH manifest output, ML
ladder prediction, and CLI chart generation.

**Not covered:** see tiers below.

---

## P0 — correctness fixes (small, do first)

- [x] **VMAF model validation.** Reject unknown VMAF model names at startup
      rather than failing deep into an encode run. Known models are validated
      against the libvmaf catalog.
- [x] **FFmpeg version detection.** Validate minimum FFmpeg/libvmaf versions at
      startup and surface clear errors instead of cryptic encode failures.
- [x] **Core algorithm tests.** Convex hull, BD-rate, Trellis allocation, and
      ladder selection are covered by a 170+ test suite, plus property-based
      (`proptest`) invariant tests for the convex hull and ladder selection.
- [x] **Integration tests.** FATE-style end-to-end tests generate synthetic
      media with `ffmpeg -f lavfi` and exercise the full probe → encode →
      measure pipeline against real ffmpeg/ffprobe.
- [x] **10-bit pipeline correctness.** 10-bit content is detected
      (`bit_depth`), preserved through encode (output pix_fmt stays 10-bit),
      and scored bit-depth-aware: `resolve_scoring_plan` keeps the native
      high-bit-depth format for VMAF/PSNR, warns when reference and distorted
      depths differ, and `psnr_peak` tracks the scoring depth. Covered by
      `fate_10bit.rs` and `viser-quality::scoring` unit tests.

## P1 — highest-value features

- [x] **Metric-by-metric comparison (MSU VQMT-class report).** Run the full
      metric suite on the same content and compare the *metrics against each
      other* — not just rank encodes — surfacing where PSNR and the perceptual
      metrics disagree. Shipped via `viser metrics compare` with a unified
      per-metric report and an agreement matrix; sub-items below.
  - [x] Per-component PSNR (Y/U/V + weighted `(6·Y + U + V) / 8`) in
        `viser-quality`.
  - [x] Pooling beyond the arithmetic mean — harmonic mean, p1/p5/p10, median,
        min/max — in `viser-quality::pool` (`PooledStats` / `PoolStrategy`).
  - [x] Metric-vs-metric correlation (Pearson, Spearman/SROCC, Kendall/KROCC)
        and divergence flagging in `viser-metrics` (`correlation_matrix`,
        `divergences`, `to_markdown`).
  - [x] Full-clip SSIMULACRA2/Butteraugli by default. `frame_samples == 0`
        (the new default) measures every frame in a single-pass batch extract;
        `--frame-samples N` stays as the speed/accuracy knob.
  - [x] Wire it into the CLI: `viser metrics compare -r ref enc_a enc_b … --all
        --pool harmonic --report {csv,json,html}` — ranks each encode per metric
        and prints the metric-vs-metric agreement on that ranking.
  - [x] Unified per-metric report (CSV/JSON/HTML) — emitted via `--report`
        (table to stdout, machine report to `--output` or stdout).
- [x] **Metric coverage parity with MSU VQMT.** Broadened the `Metric` enum
      (and the `Result`/`Pooled`/`FrameResult` structs) past VMAF/PSNR/SSIM/
      SSIMULACRA2/Butteraugli, all wired into `metrics compare`.
  - [x] **MS-SSIM** — multi-scale SSIM via libvmaf's `float_ms_ssim`; rides the
        existing VMAF pass.
  - [x] **VIF** — visual information fidelity, the mean of libvmaf's
        `*_vif_scale0..3`; computed alongside VMAF for free.
  - [x] **XPSNR** — perceptually-weighted PSNR `(6·Y+U+V)/8` via FFmpeg's
        `xpsnr` filter (a separate pass; under `--all`).
  - [x] **CAMBI** — Netflix's banding detector via libvmaf's `cambi` feature
        (lower is better; oriented "up" in the agreement matrix).
- [x] **Two-pass VBR encoding.** `RateControlMode::Vbr` runs a two-pass encode
      against a target bitrate (`encode_two_pass`), alongside CRF, capped-CRF,
      and fixed-QP modes; per-title delivery maps saved analyses to VBR rungs.
- [~] **HDR support (proper).** PQ/HLG handling, HDR-aware VMAF models.
  - [x] HDR10 static-metadata preservation. `viser-ffmpeg::hdr` extracts
        mastering-display colour volume (SMPTE ST 2086) and MaxCLL/MaxFALL from
        the source's frame side data; `SourceFormat::enrich_hdr10` attaches it
        across the per-title/per-segment/delivery pipelines. x265 re-signals it
        via `master-display` / `max-cll`, and SVT-AV1 via `-svtav1-params`
        `master-display` / `content-light` (real-valued grammar, with the
        rate-control `-svtav1-params` coalesced). FATE round-trips verify both
        codecs survive a re-encode.
  - [x] HDR-aware scoring via tonemap-to-BT.709 (`--hdr-scoring`), shipped with
        the 0.9.0 10-bit/HDR work.
  - [x] **Native HDR-domain VMAF + HW encoder HDR10 passthrough.** Two sub-items
        that ship together in 0.12.0.
    - [x] **HW encoder high bit depth.** NVENC, QSV, AMF, VideoToolbox, and VAAPI
          all receive `-pix_fmt p010le` (or `p010` VAAPI surface) when the source
          is 10-bit; `-profile:v main10` is emitted for HEVC/AV1 backends.
          `codec_supports_bit_depth` extended to cover all HW codecs.
    - [x] **HW encoder HDR metadata.** For HEVC and H.264 hardware encoders,
          mastering-display and MaxCLL/MaxFALL are injected via codec-family
          bitstream filters (`hevc_metadata` / `h264_metadata`), which operate on
          the encoded bitstream regardless of the encoder backend. AV1 hardware
          encoders (that lack BSF support) continue to use `-color_*` tags.
    - [x] **VAAPI HDR surfaces.** `build_filter_chain()` emits `format=p010`
          instead of `format=nv12` when the source is HDR or high-bit-depth.
    - [x] **Custom VMAF model paths.** `validate_vmaf_model` and the libvmaf
          filter string accept `.cfg`/`.json`/`.model` file paths (using
          `model=path=...` syntax) in addition to built-in model names, enabling
          users to supply third-party HDR-domain VMAF models alongside
          `--hdr-scoring hdr-native`.
- [x] **Chunked/segmented encoding.** Encode long-form content by splitting into
      independent chunks. `viser-ffmpeg` gains `chunk_plan()` and `chunked_encode()`
      (parallel chunk encode + automatic concat). The `encode` CLI command accepts
      `--chunk-seconds` and `--parallel` for chunked single-output encodes. The
      delivery pipeline delegates to the shared `chunk_plan()`. Distributed
      multi-machine coordination remains in Future.
- [x] **Scene-complexity blending.** `viser-ladder::blend_shot_ladders` merges
      per-shot hulls into a duration-weighted composite ladder with optional
      `smooth_ladder` transition capping. CLI: `per-shot analyze --blend-ladder`.

## P2 — completeness

- [x] **Differentiators beyond MSU VQMT parity.** Builds on the metric-
      comparison work in P1, but reaches past what MSU/psy-ex/ffmpeg-quality-
      metrics offer.
  - [x] **No-reference metrics.** `metrics no-ref` scores files with no pristine
        source: model-free signals (sharpness, blockiness, noise) plus trained
        NIQE/BRISQUE in `viser-quality`, streamed frame-by-frame from a `gray8`
        pipe.
    - [x] **NIQE / BRISQUE proper.** Embedded reference models — utlive NIQE
          MVG (`modelparameters.mat` → `niqe_model.json`) and OpenCV BRISQUE
          EPS-SVR (`brisque_model_live` + `brisque_range_live` →
          `brisque_model.json`). Pure-Rust feature extraction + inference;
          wired into `metrics no-ref` as NIQE/BRISQUE columns (lower is better).
  - [~] **Faithfulness / hallucination metric (research-grade).** Distinguish
        recovered detail from *invented* detail in AI-enhanced output.
        - [x] **v0 heuristic.** `metrics faithfulness` scores HF gain, texture-
              paradox, and optional VMAF/PSNR paradox between reference and
              distorted pairs (`viser-quality::faithfulness`).
        - [ ] **v1+ research.** Seed-disagreement heatmaps, round-trip re-
              degradation consistency, frequency-band attribution, and
              validation against known AI-enhancement corpora.
  - [~] **Pure-Rust + WASM measurement.** Replace the libvmaf/FFmpeg/CLI shell-
        outs with native implementations so the comparison player computes and
        overlays metrics in the browser.
        - [x] **WASM foundation.** `viser-wasm` exports gray8 frame scoring
              (sharpness, blockiness, noise, NIQE, BRISQUE) via wasm-bindgen.
        - [ ] **Browser decode path.** WebCodecs/canvas frame feed into WASM
              scorer; wire into the comparison player overlay.
        - [ ] **Native VMAF/PSNR in WASM.** libvmaf remains CPU-only and large;
              needs a separate effort (or server-side fallback).
  - [x] **HDR-aware metric variants.** `--hdr-scoring hdr-native` keeps PQ/HLG
        transfer in the scoring filtergraph (no BT.709 tonemap) for PSNR/SSIM/
        VMAF passes; 10-bit depth preserved. VMAF still uses SDR models — scores
        are best-effort until an official HDR VMAF model ships.
- [x] **Hardware encode/decode matrix.** NVENC, QuickSync, VideoToolbox, VAAPI,
      AMF integration for GPU-accelerated encodes across H.264, H.265, and AV1,
      plus hardware-accelerated decode. Shipped in 0.6.0 (H.264/H.265 encode),
      completed in 0.7.0 (AV1 encode row + VAAPI surface plumbing + decode axis):

  - [x] *Runtime detection.* `ffmpeg -encoders` and `ffmpeg -hwaccels` probed at
        CLI startup; available encoders/decoders cached in `OnceLock` sets.
  - [x] *Codec enum.* 17 variants: 3 SW + 14 HW. H.264/H.265 across all 5
        backends, plus AV1 across NVENC/QSV/VAAPI/AMF (no `av1_videotoolbox` —
        Apple has no AV1 encoder). `EncoderBackend`, `CodecFamily`, `backend()`,
        `family()`, `is_hardware()`, `is_software()`.
  - [x] *Rate-control dispatch.* `build_sw_args()` / `build_hw_args()` in
        `viser-ffmpeg/src/encode.rs` with per-backend rate-control flags
        (NVENC `-cq -rc constqp`, QSV `-global_quality`, VideoToolbox
        `-quality`, VAAPI `-global_quality`, AMF `-qp_i / -qp_p`). Backend-keyed,
        so the AV1 row reuses the existing dispatch.
  - [x] *VAAPI surface plumbing.* `-vaapi_device` initialised before `-i`
        (overridable via `VISER_VAAPI_DEVICE`), and a unified `-vf` filter chain
        appends `format=nv12,hwupload` so the encoder receives VAAPI surfaces.
  - [x] *Hardware decode.* `EncodeJob.hwaccel` injects `-hwaccel <method>` before
        the input (frames downloaded to system memory, keeping the SW filter
        pipeline intact). `encode --hwaccel` flag; detection via `-hwaccels`.
  - [x] *Preset mapping.* NVENC `p1`-`p7`, QSV passthrough, VAAPI
        `compression_level` 1-5, AMF `speed/balanced/quality`, in
        `viser-encoding/src/lib.rs`.
  - [x] *CLI integration.* All commands (`per-title analyze`, `per-title
        deliver`, `per-shot analyze`, `per-segment analyze`, `encode`) accept
        HW codec names and aliases (`nvenc`, `qsv`, `vt`, `vaapi`, `amf`,
        `videotoolbox`, plus `av1_nvenc` / `av1_qsv` / `av1_vaapi` / `av1_amf`).
  - [x] *Chart labels.* `viser-chart` maps all 14 HW encoder names to
        human-readable labels (e.g., `h264_nvenc` → `H.264 (NVENC)`,
        `av1_vaapi` → `AV1 (VAAPI)`).

  **Scope boundary.** No native FFI bindings — all HW encode/decode goes through
  the FFmpeg subprocess. AV1 HW encode requires recent silicon (Arc/Battlemage,
  Ada/Blackwell, RDNA3+) and is validated at the argument level; real-GPU
  validation needs hardware in CI. GPU-accelerated VMAF remains deferred
  (libvmaf is CPU-only; no viable GPU path exists).
- [x] **VP9 codec support.** `libvpx-vp9` wired through the `Codec` enum with
      VP9-specific preset mapping (`-cpu-used` / `-deadline good`), constrained-
      quality capped CRF (`-b:v` + `-crf`), and 10-bit preservation. CLI aliases:
      `vp9`, `libvpx-vp9`, `libvpx`.
- [x] **ML-based ladder prediction.** `viser-predict` extracts complexity
      features and predicts R-D points with calibrated heuristics (codec-efficiency
      + spatial/temporal factors), then runs hull + ladder selection — no trial
      encodes. CLI: `viser per-title predict -i source.mp4 -o prediction.json`.
- [x] **Streaming manifest output.** HLS master playlists and static DASH MPDs
      from delivery rungs via `viser-ladder::manifest`. `per-title deliver`
      accepts `--hls-manifest`, `--dash-manifest`, and `--manifest-base-url`.
- [x] **Audio bitrate optimization.** Per-title analysis extracts source audio
      bitrate (`audio_bitrate_kbps`) and reserves it in the delivery budget, so
      ladder rungs are sized against the video budget alone.
- [x] **Screen content detection.** `viser-complexity::detect_screen_content`
      classifies content as natural vs. screen (slides, code, UI) from
      spatial/temporal/DCT heuristics. `encoding_hints` + `apply_encoding_hints`
      auto-adjust CRF sweep and preset in `per-title analyze` / `predict` when
      screen content is detected.

## P3 — quality of life

- [x] **Charts in CLI.** `per-title analyze --charts <dir>` and `viser chart
      --analysis result.json --output <dir>` emit R-D, per-codec, and ladder PNGs
      via `viser-chart`. Per-shot blended-ladder chart via `--blend-ladder
      --charts`.
- [x] **Cost-aware optimization.** Storage + CDN delivery costs factored into
      ladder selection. `CostOpts` struct with per-GB storage/CDN pricing, viewing
      hours, and a monthly budget cap. `Ladder::monthly_cost()` estimates total
      spend (storage for all rungs + delivery at the top rung). When
      `--max-monthly-cost` is set, `select()` prunes the ladder after initial
      selection by iteratively removing the rung with the worst cost-per-VMAF
      ratio until the budget is met. CLI: `--storage-cost`, `--cdn-cost`,
      `--viewing-hours`, `--max-monthly-cost`.
- [x] **ABR logic integration.** Ladder selection now supports a bitrate-target
      mode alongside the existing VMAF-target mode. When `--abr-bitrates` is set,
      rungs are placed at the specified bitrates by greedily matching the closest
      hull point (crossover/VMAF constraints still apply). A `logarithmic_bitrates`
      helper generates spacing that matches industry ABR spec behavior (denser at
      low bitrates). CLI: `per-title analyze --abr-bitrates 300,600,1200,2500,5000`
      or `--abr-logarithmic`. Shipped as `AbrOpts` in `viser-ladder`.

## Future — worth tracking

- [x] **NIQE/BRISQUE differential validation.** Frame generators produce a
      fixed corpus of synthetic gray8 frames (uniform, checkerboard, gradient,
      white noise). NIQE and BRISQUE tests validate finite bounds, relative
      ordering (structured > uniform), and gate against OpenCV reference scores
      via `eprintln!` warnings when drift exceeds tolerance. Both model files
      include comments with the OpenCV commands needed to regenerate references.
- [x] **ExtraTrees / ONNX ladder predictor.** `tract-onnx` optional dependency
      (`onnx` feature) loads an ExtraTrees model exported to ONNX for R-D point
      prediction. The `OnnxModel` in `viser-predict/src/onnx.rs` takes 7 features
      (complexity, spatial, temporal, log-pixels, codec-efficiency, CRF, audio)
      and returns `(bitrate, vmaf)`. Falls back to heuristics when no model is
      provided or loading fails. CLI: `--predict-model <path>` on `per-title
      predict`. Training script at `train/train_extra_trees.py` converts per-title
      analysis JSON → ONNX via sklearn ExtraTreesRegressor.
- [ ] **Faithfulness heatmaps.** Per-frame HF-gain maps overlaid in the
      comparison player for AI-enhancement QA workflows.
- [ ] **Distributed encode coordinator.** Job queue + object-store artifacts for
      chunked per-title trial matrices (feeds the P1 chunked-encoding item).
- [x] **Screen-content tune presets per codec.** `screen_content_encoder_args()`
      in `viser-complexity` returns codec-specific FFmpeg flags: `-tune-content
      screen` for x264, `enable-qm=1:enable-overlays=1` for SVT-AV1, and empty
      for x265/HW encoders (CRF+preset adjustments suffice). Wired into the
      `extra_encoder_args` field on per-title and per-shot Config, which flows
      into each `EncodeJob.extra_args` during trial encoding.