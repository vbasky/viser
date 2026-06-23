mod common;

use common::{generate_10bit_sdr_clip, has_ffmpeg, has_libvmaf};
use viser_ffmpeg::{Codec, EncodeJob, RateControlMode, SourceFormat, bit_depth, encode, probe};

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
