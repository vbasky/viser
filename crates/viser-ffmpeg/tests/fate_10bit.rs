mod common;

use common::{generate_10bit_sdr_clip, generate_hdr10_clip, has_encoder, has_ffmpeg, has_libvmaf};
use viser_ffmpeg::{
    Codec, EncodeJob, RateControlMode, SourceFormat, bit_depth, encode, probe, probe_hdr10_metadata,
};

#[tokio::test]
async fn fate_encode_preserves_10bit_pix_fmt() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let source = generate_10bit_sdr_clip(tmp.path());
    let source_info = probe(&source.to_string_lossy()).await.unwrap();
    let video = source_info.video_stream().unwrap();
    assert_eq!(bit_depth(video), 10);

    let output = tmp.path().join("encoded_10bit.mp4");
    let job = EncodeJob {
        input: source.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X265,
        crf: 28,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        hwaccel: None,
        extra_args: vec![],
        source_format: Some(SourceFormat::from_stream(video)),
    };

    encode(job, None).await.unwrap();

    let out_info = probe(&output.to_string_lossy()).await.unwrap();
    let out_video = out_info.video_stream().unwrap();
    assert!(
        out_video.pix_fmt.contains("10"),
        "expected 10-bit output pix_fmt, got {}",
        out_video.pix_fmt
    );
}

#[tokio::test]
async fn fate_quality_10bit_vmaf_ordering() {
    if !has_ffmpeg() || !has_libvmaf() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let source = generate_10bit_sdr_clip(tmp.path());
    let source_info = probe(&source.to_string_lossy()).await.unwrap();
    let video = source_info.video_stream().unwrap();
    let source_format = SourceFormat::from_stream(video);

    async fn encode_at(
        source: &str,
        dir: &std::path::Path,
        crf: i32,
        format: &SourceFormat,
    ) -> std::path::PathBuf {
        let output = dir.join(format!("crf_{crf}.mp4"));
        let job = EncodeJob {
            input: source.to_string(),
            output: output.to_string_lossy().into_owned(),
            resolution: None,
            codec: Codec::X265,
            crf,
            rate_control: RateControlMode::Crf,
            target_bitrate: 0.0,
            max_bitrate: 0.0,
            bufsize: 0.0,
            preset: "ultrafast".into(),
            hwaccel: None,
            extra_args: vec![],
            source_format: Some(format.clone()),
        };
        encode(job, None).await.unwrap();
        output
    }

    let source_str = source.to_string_lossy().into_owned();
    let high = encode_at(&source_str, tmp.path(), 22, &source_format).await;
    let low = encode_at(&source_str, tmp.path(), 40, &source_format).await;

    let opts = viser_quality::MeasureOpts {
        metrics: vec![viser_quality::Metric::Vmaf],
        subsample: 0,
        model: "vmaf_v0.6.1".into(),
        per_frame: false,
        frame_samples: 0,
        probe_cache: None,
        hdr_scoring: viser_quality::HdrScoringMode::Auto,
    };

    let vmaf_high =
        viser_quality::measure(&source_str, &high.to_string_lossy(), opts.clone()).await.unwrap();
    let vmaf_low = viser_quality::measure(&source_str, &low.to_string_lossy(), opts).await.unwrap();

    assert!(
        vmaf_high.vmaf > vmaf_low.vmaf,
        "CRF 22 ({}) should beat CRF 40 ({}) on 10-bit SDR",
        vmaf_high.vmaf,
        vmaf_low.vmaf
    );
}

#[tokio::test]
async fn fate_probe_extracts_hdr10_metadata() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let source = generate_hdr10_clip(tmp.path());

    let md = probe_hdr10_metadata(&source.to_string_lossy())
        .await
        .unwrap()
        .expect("HDR10 metadata should be present on the source");

    let display = md.mastering_display.expect("mastering display present");
    assert_eq!(display.green_x, 13250);
    assert_eq!(display.max_luminance, 10_000_000);
    assert_eq!(md.max_cll, Some(1000));
    assert_eq!(md.max_fall, Some(400));
}

#[tokio::test]
async fn fate_encode_preserves_hdr10_metadata() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let source = generate_hdr10_clip(tmp.path());
    let source_str = source.to_string_lossy().into_owned();

    let source_info = probe(&source_str).await.unwrap();
    let video = source_info.video_stream().unwrap();
    let source_format = SourceFormat::from_stream(video).enrich_hdr10(&source_str).await;
    assert!(source_format.is_hdr, "source should be detected as HDR");
    assert!(source_format.hdr10.is_some(), "HDR10 metadata should be attached");

    let output = tmp.path().join("encoded_hdr10.mp4");
    let job = EncodeJob {
        input: source_str,
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X265,
        crf: 28,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        hwaccel: None,
        extra_args: vec![],
        source_format: Some(source_format),
    };

    encode(job, None).await.unwrap();

    // The re-encode must carry the mastering-display and content-light side data
    // through to the output, not just the transfer/primaries signalling.
    let out_md = probe_hdr10_metadata(&output.to_string_lossy())
        .await
        .unwrap()
        .expect("HDR10 metadata should survive the re-encode");
    let display = out_md.mastering_display.expect("mastering display preserved");
    assert_eq!(display.green_x, 13250);
    assert_eq!(display.max_luminance, 10_000_000);
    assert_eq!(out_md.max_cll, Some(1000));
    assert_eq!(out_md.max_fall, Some(400));
}

#[tokio::test]
async fn fate_svtav1_encode_preserves_hdr10_metadata() {
    if !has_ffmpeg() || !has_encoder("libsvtav1") {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let source = generate_hdr10_clip(tmp.path());
    let source_str = source.to_string_lossy().into_owned();

    let source_info = probe(&source_str).await.unwrap();
    let video = source_info.video_stream().unwrap();
    let source_format = SourceFormat::from_stream(video).enrich_hdr10(&source_str).await;
    assert!(source_format.hdr10.is_some(), "HDR10 metadata should be attached");

    let output = tmp.path().join("encoded_hdr10_av1.mp4");
    let job = EncodeJob {
        input: source_str,
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::SvtAv1,
        crf: 40,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "8".into(),
        hwaccel: None,
        extra_args: vec![],
        source_format: Some(source_format),
    };

    encode(job, None).await.unwrap();

    let out_md = probe_hdr10_metadata(&output.to_string_lossy())
        .await
        .unwrap()
        .expect("HDR10 metadata should survive the SVT-AV1 re-encode");
    let display = out_md.mastering_display.expect("mastering display preserved");

    // AV1 re-quantises chromaticity into 1/65536 and luminance into 1/16384
    // units, so the values normalise back to within a couple of the x265-scaled
    // originals (the source clip is mastered at G(0.265,0.69)…L(1000 nits, min)).
    let near = |actual: u32, expected: u32, tol: u32| {
        assert!(actual.abs_diff(expected) <= tol, "expected ~{expected} (±{tol}), got {actual}");
    };
    near(display.green_x, 13250, 4);
    near(display.green_y, 34500, 4);
    near(display.red_x, 34000, 4);
    near(display.max_luminance, 10_000_000, 1000);
    assert_eq!(out_md.max_cll, Some(1000));
    assert_eq!(out_md.max_fall, Some(400));
}
