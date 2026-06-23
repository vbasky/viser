mod common;

use common::{generate_reference_clip, has_ffmpeg, has_libvmaf};
use viser_ffmpeg::{Codec, EncodeJob, RateControlMode, encode};

/// Helper: encode at a given CRF and return the output path.
async fn encode_at_crf(input: &str, output_dir: &std::path::Path, crf: i32) -> std::path::PathBuf {
    let output = output_dir.join(format!("crf_{crf}.mp4"));
    let job = EncodeJob {
        input: input.to_string(),
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X264,
        crf,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        hwaccel: None,
        extra_args: vec![],
        source_format: None,
    };
    encode(job, None).await.unwrap();
    output
}

#[tokio::test]
async fn fate_quality_vmaf_lossless_should_be_high() {
    if !has_ffmpeg() || !has_libvmaf() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let reference = generate_reference_clip(tmp.path(), "ref_vmaf.mp4", "640x360", 2);
    let distorted = encode_at_crf(&reference.to_string_lossy(), tmp.path(), 18).await;

    let opts = viser_quality::MeasureOpts {
        metrics: vec![viser_quality::Metric::Vmaf],
        subsample: 0,
        model: "vmaf_v0.6.1".into(),
        per_frame: false,
        frame_samples: 0,
        probe_cache: None,
        ..Default::default()
    };

    let result =
        viser_quality::measure(&reference.to_string_lossy(), &distorted.to_string_lossy(), opts)
            .await
            .unwrap();

    // CRF 18 should produce very high VMAF (typically > 90)
    assert!(result.vmaf > 80.0, "CRF 18 VMAF should be > 80, got {}", result.vmaf);
    assert!(result.vmaf <= 100.0, "VMAF should be <= 100, got {}", result.vmaf);
}

#[tokio::test]
async fn fate_quality_vmaf_lower_crf_higher_score() {
    if !has_ffmpeg() || !has_libvmaf() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let reference = generate_reference_clip(tmp.path(), "ref_vmaf2.mp4", "640x360", 2);

    let high_quality = encode_at_crf(&reference.to_string_lossy(), tmp.path(), 18).await;
    let low_quality = encode_at_crf(&reference.to_string_lossy(), tmp.path(), 42).await;

    let base_opts = viser_quality::MeasureOpts {
        metrics: vec![viser_quality::Metric::Vmaf],
        subsample: 0,
        model: "vmaf_v0.6.1".into(),
        per_frame: false,
        frame_samples: 0,
        probe_cache: None,
        ..Default::default()
    };

    let vmaf_high = viser_quality::measure(
        &reference.to_string_lossy(),
        &high_quality.to_string_lossy(),
        base_opts.clone(),
    )
    .await
    .unwrap();

    let vmaf_low = viser_quality::measure(
        &reference.to_string_lossy(),
        &low_quality.to_string_lossy(),
        base_opts,
    )
    .await
    .unwrap();

    assert!(
        vmaf_high.vmaf > vmaf_low.vmaf,
        "CRF 18 ({}) should have higher VMAF than CRF 42 ({})",
        vmaf_high.vmaf,
        vmaf_low.vmaf
    );
}

#[tokio::test]
async fn fate_quality_psnr_is_computed() {
    if !has_ffmpeg() || !has_libvmaf() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let reference = generate_reference_clip(tmp.path(), "ref_psnr.mp4", "640x360", 2);
    let distorted = encode_at_crf(&reference.to_string_lossy(), tmp.path(), 18).await;

    let opts = viser_quality::MeasureOpts {
        metrics: vec![viser_quality::Metric::Psnr],
        subsample: 0,
        model: "vmaf_v0.6.1".into(),
        per_frame: false,
        frame_samples: 0,
        probe_cache: None,
        ..Default::default()
    };

    let result =
        viser_quality::measure(&reference.to_string_lossy(), &distorted.to_string_lossy(), opts)
            .await
            .unwrap();

    // CRF 18 should have decent PSNR
    assert!(result.psnr > 30.0, "CRF 18 PSNR should be > 30, got {}", result.psnr);
}

#[tokio::test]
async fn fate_quality_ssim_is_computed() {
    if !has_ffmpeg() || !has_libvmaf() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let reference = generate_reference_clip(tmp.path(), "ref_ssim.mp4", "640x360", 2);
    let distorted = encode_at_crf(&reference.to_string_lossy(), tmp.path(), 18).await;

    let opts = viser_quality::MeasureOpts {
        metrics: vec![viser_quality::Metric::Ssim],
        subsample: 0,
        model: "vmaf_v0.6.1".into(),
        per_frame: false,
        frame_samples: 0,
        probe_cache: None,
        ..Default::default()
    };

    let result =
        viser_quality::measure(&reference.to_string_lossy(), &distorted.to_string_lossy(), opts)
            .await
            .unwrap();

    assert!(result.ssim > 0.8, "CRF 18 SSIM should be > 0.8, got {}", result.ssim);
    assert!(result.ssim <= 1.0, "SSIM should be <= 1.0, got {}", result.ssim);
}

#[tokio::test]
async fn fate_quality_per_frame_data() {
    if !has_ffmpeg() || !has_libvmaf() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let reference = generate_reference_clip(tmp.path(), "ref_perframe.mp4", "640x360", 2);
    let distorted = encode_at_crf(&reference.to_string_lossy(), tmp.path(), 23).await;

    let opts = viser_quality::MeasureOpts {
        metrics: vec![viser_quality::Metric::Vmaf],
        subsample: 0,
        model: "vmaf_v0.6.1".into(),
        per_frame: true,
        frame_samples: 0,
        probe_cache: None,
        ..Default::default()
    };

    let result =
        viser_quality::measure(&reference.to_string_lossy(), &distorted.to_string_lossy(), opts)
            .await
            .unwrap();

    assert!(!result.frames.is_empty(), "per_frame should produce frame data");
    for f in &result.frames {
        assert!(
            f.vmaf >= 0.0 && f.vmaf <= 100.0,
            "frame {} VMAF {} out of range",
            f.frame_num,
            f.vmaf
        );
    }
}

#[tokio::test]
async fn fate_quality_default_metrics() {
    if !has_ffmpeg() || !has_libvmaf() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let reference = generate_reference_clip(tmp.path(), "ref_default.mp4", "640x360", 2);
    let distorted = encode_at_crf(&reference.to_string_lossy(), tmp.path(), 23).await;

    let opts = viser_quality::MeasureOpts::default();
    let result =
        viser_quality::measure(&reference.to_string_lossy(), &distorted.to_string_lossy(), opts)
            .await
            .unwrap();

    assert!(result.vmaf > 0.0, "default should compute VMAF");
    assert!(result.psnr > 0.0, "default should compute PSNR");
}

#[tokio::test]
async fn fate_quality_probe_cache_reuse() {
    if !has_ffmpeg() || !has_libvmaf() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let reference = generate_reference_clip(tmp.path(), "ref_cache.mp4", "640x360", 2);
    let distorted = encode_at_crf(&reference.to_string_lossy(), tmp.path(), 23).await;

    let cache = viser_ffmpeg::ProbeCache::new();

    let opts = viser_quality::MeasureOpts {
        metrics: vec![viser_quality::Metric::Vmaf],
        subsample: 0,
        model: "vmaf_v0.6.1".into(),
        per_frame: false,
        frame_samples: 0,
        probe_cache: Some(cache.clone()),
        ..Default::default()
    };

    let result1 = viser_quality::measure(
        &reference.to_string_lossy(),
        &distorted.to_string_lossy(),
        opts.clone(),
    )
    .await
    .unwrap();

    let result2 =
        viser_quality::measure(&reference.to_string_lossy(), &distorted.to_string_lossy(), opts)
            .await
            .unwrap();

    assert!(
        (result1.vmaf - result2.vmaf).abs() < 1e-9,
        "cached probe should produce same VMAF: {} vs {}",
        result1.vmaf,
        result2.vmaf
    );
}

#[tokio::test]
async fn fate_quality_no_ref_signal_computation() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_reference_clip(tmp.path(), "ref_noref.mp4", "640x360", 2);

    let opts = viser_quality::NoRefOpts { stride: 0, probe_cache: None };
    let result = viser_quality::measure_noref(&clip.to_string_lossy(), &opts).await.unwrap();

    // Test pattern should have moderate sharpness (not zero)
    assert!(result.sharpness > 0.0, "test pattern should have sharpness > 0");
}

#[tokio::test]
async fn fate_quality_error_for_missing_reference() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_reference_clip(tmp.path(), "ref_err.mp4", "640x360", 1);

    let opts = viser_quality::MeasureOpts {
        metrics: vec![viser_quality::Metric::Vmaf],
        subsample: 0,
        model: "vmaf_v0.6.1".into(),
        per_frame: false,
        frame_samples: 0,
        probe_cache: None,
        ..Default::default()
    };

    let result =
        viser_quality::measure("/nonexistent/ref_12345.mp4", &clip.to_string_lossy(), opts).await;

    assert!(result.is_err(), "measuring with missing reference should error");
}
