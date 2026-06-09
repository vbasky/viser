mod common;

use common::{generate_reference_clip, generate_test_clip, has_ffmpeg};
use tokio::sync::mpsc;
use viser_ffmpeg::{Codec, EncodeJob, RateControlMode, Resolution, concat, encode, extract, probe};

#[tokio::test]
async fn fate_encode_x264_crf_produces_output() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_crf.mp4", "640x360", 2, 24, "libx264");
    let output = tmp.path().join("encoded_crf.mp4");

    let job = EncodeJob {
        input: input.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X264,
        crf: 23,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        extra_args: vec![],
    };

    let result = encode(job, None).await.unwrap();
    assert!(output.exists(), "encoded output must exist");
    assert!(result.file_size > 0, "encoded file must have size > 0");
    assert!(result.bitrate > 0.0, "bitrate must be > 0");

    // Verify output is valid h264
    let probed = probe(&output.to_string_lossy()).await.unwrap();
    let video = probed.video_stream().unwrap();
    assert_eq!(video.codec_name, "h264");
}

#[tokio::test]
async fn fate_encode_x265_crf() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_x265.mp4", "640x360", 1, 24, "libx264");
    let output = tmp.path().join("encoded_x265.mp4");

    let job = EncodeJob {
        input: input.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X265,
        crf: 28,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        extra_args: vec![],
    };

    let result = encode(job, None).await.unwrap();
    assert!(output.exists());
    assert!(result.file_size > 0);

    let probed = probe(&output.to_string_lossy()).await.unwrap();
    let video = probed.video_stream().unwrap();
    assert_eq!(video.codec_name, "hevc");
}

#[tokio::test]
async fn fate_encode_with_resolution_scaling() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_scale.mp4", "1280x720", 1, 24, "libx264");
    let output = tmp.path().join("scaled_480p.mp4");

    let job = EncodeJob {
        input: input.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        resolution: Some(Resolution::new(640, 480)),
        codec: Codec::X264,
        crf: 23,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        extra_args: vec![],
    };

    encode(job, None).await.unwrap();
    let probed = probe(&output.to_string_lossy()).await.unwrap();
    let video = probed.video_stream().unwrap();
    assert_eq!(video.width, 640);
    assert_eq!(video.height, 480);
}

#[tokio::test]
async fn fate_encode_crf_higher_produces_smaller_file() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_compare.mp4", "640x360", 2, 24, "libx264");

    let mut sizes = Vec::new();
    for crf in &[18, 30, 42] {
        let output = tmp.path().join(format!("crf_{crf}.mp4"));
        let job = EncodeJob {
            input: input.to_string_lossy().into_owned(),
            output: output.to_string_lossy().into_owned(),
            resolution: None,
            codec: Codec::X264,
            crf: *crf,
            rate_control: RateControlMode::Crf,
            target_bitrate: 0.0,
            max_bitrate: 0.0,
            bufsize: 0.0,
            preset: "ultrafast".into(),
            extra_args: vec![],
        };
        let result = encode(job, None).await.unwrap();
        sizes.push((*crf, result.file_size));
    }

    // Higher CRF = lower quality = lower bitrate = smaller file
    assert!(
        sizes[0].1 > sizes[1].1,
        "CRF 18 ({}) should be bigger than CRF 30 ({})",
        sizes[0].1,
        sizes[1].1
    );
    assert!(
        sizes[1].1 > sizes[2].1,
        "CRF 30 ({}) should be bigger than CRF 42 ({})",
        sizes[1].1,
        sizes[2].1
    );
}

#[tokio::test]
async fn fate_encode_progress_reporting() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_prog.mp4", "640x360", 2, 24, "libx264");
    let output = tmp.path().join("encoded_prog.mp4");

    let (tx, mut rx) = mpsc::channel::<viser_ffmpeg::Progress>(32);

    let job = EncodeJob {
        input: input.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X264,
        crf: 23,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        extra_args: vec![],
    };

    let handle = tokio::spawn(async move { encode(job, Some(tx)).await.unwrap() });

    let mut progress_count = 0;
    while let Some(p) = rx.recv().await {
        progress_count += 1;
        assert!(p.frame >= 0, "frame should be >= 0");
        assert!(p.fps >= 0.0, "fps should be >= 0");
        assert!(p.speed >= 0.0, "speed should be >= 0");
    }

    handle.await.unwrap();
    assert!(progress_count > 0, "should receive at least one progress update");
}

#[tokio::test]
async fn fate_extract_segment() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_extract.mp4", "1280x720", 3, 30, "libx264");
    let output = tmp.path().join("segment.mp4");

    extract(&input.to_string_lossy(), &output.to_string_lossy(), 0.5, 1.0).await.unwrap();

    assert!(output.exists(), "extracted segment must exist");

    let probed = probe(&output.to_string_lossy()).await.unwrap();
    assert!(
        probed.format.duration > 0.5,
        "extracted duration too short: {}",
        probed.format.duration
    );
    assert!(
        probed.format.duration < 2.5,
        "extracted duration too long: {}",
        probed.format.duration
    );
}

#[tokio::test]
async fn fate_extract_segment_preserves_codec() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_copy.mp4", "640x360", 3, 24, "libx264");
    let output = tmp.path().join("segment_copy.mp4");

    extract(&input.to_string_lossy(), &output.to_string_lossy(), 0.0, 1.0).await.unwrap();

    let original = probe(&input.to_string_lossy()).await.unwrap();
    let extracted = probe(&output.to_string_lossy()).await.unwrap();

    assert_eq!(
        original.video_stream().unwrap().codec_name,
        extracted.video_stream().unwrap().codec_name,
        "extract should preserve codec"
    );
}

#[tokio::test]
async fn fate_concat_multiple_segments() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_concat.mp4", "320x240", 3, 30, "libx264");

    let seg1 = tmp.path().join("seg1.mp4");
    let seg2 = tmp.path().join("seg2.mp4");
    let output = tmp.path().join("concatenated.mp4");

    extract(&input.to_string_lossy(), &seg1.to_string_lossy(), 0.0, 1.0).await.unwrap();
    extract(&input.to_string_lossy(), &seg2.to_string_lossy(), 1.0, 1.0).await.unwrap();

    let inputs = vec![seg1.to_string_lossy().into_owned(), seg2.to_string_lossy().into_owned()];
    concat(&inputs, &output.to_string_lossy()).await.unwrap();

    assert!(output.exists(), "concatenated output must exist");

    let probed = probe(&output.to_string_lossy()).await.unwrap();
    assert!(probed.format.duration > 1.0, "concat duration too short: {}", probed.format.duration);
    assert!(probed.format.duration < 5.0, "concat duration too long: {}", probed.format.duration);
}

#[tokio::test]
async fn fate_concat_empty_input_list_errors() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("empty_concat.mp4");
    let result = concat(&[], &output.to_string_lossy()).await;
    assert!(result.is_err(), "concat with empty input list should error");
}

#[tokio::test]
async fn fate_encode_error_on_nonexistent_input() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let job = EncodeJob {
        input: "/nonexistent/input_12345.mp4".into(),
        output: tmp.path().join("should_not_exist.mp4").to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X264,
        crf: 23,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        extra_args: vec![],
    };

    let result = encode(job, None).await;
    assert!(result.is_err(), "encoding nonexistent input should error");
}

#[tokio::test]
async fn fate_encode_crf_zero_produces_large_file() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_q0.mp4", "320x240", 1, 24, "libx264");
    let output = tmp.path().join("crf0.mp4");

    let job = EncodeJob {
        input: input.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X264,
        crf: 0,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        extra_args: vec![],
    };

    let result = encode(job, None).await.unwrap();
    assert!(result.file_size > 1000, "lossless CRF 0 should produce non-trivial file");
}

#[tokio::test]
async fn fate_encode_x264_qp() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_qp.mp4", "640x360", 1, 24, "libx264");
    let output = tmp.path().join("qp_encoded.mp4");

    let job = EncodeJob {
        input: input.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X264,
        crf: 23,
        rate_control: RateControlMode::Qp,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        extra_args: vec![],
    };

    let result = encode(job, None).await.unwrap();
    assert!(result.file_size > 0);
    assert!(output.exists());
}

#[tokio::test]
async fn fate_encode_capped_crf() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_capped.mp4", "640x360", 1, 24, "libx264");
    let output = tmp.path().join("capped_crf.mp4");

    let job = EncodeJob {
        input: input.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X264,
        crf: 23,
        rate_control: RateControlMode::CappedCrf,
        target_bitrate: 0.0,
        max_bitrate: 2000.0,
        bufsize: 4000.0,
        preset: "ultrafast".into(),
        extra_args: vec![],
    };

    let result = encode(job, None).await.unwrap();
    assert!(result.file_size > 0);
    assert!(
        result.bitrate <= 2500.0,
        "capped CRF bitrate {} exceeds max 2000kbps + margin",
        result.bitrate
    );
}

#[tokio::test]
async fn fate_encode_preserves_no_audio() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_noaud.mp4", "640x360", 1, 24, "libx264");
    let output = tmp.path().join("no_audio.mp4");

    let job = EncodeJob {
        input: input.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X264,
        crf: 23,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        extra_args: vec![],
    };

    encode(job, None).await.unwrap();
    let probed = probe(&output.to_string_lossy()).await.unwrap();
    assert!(probed.audio_stream().is_none(), "encode with -an should strip audio stream");
}

#[tokio::test]
async fn fate_encode_with_extra_args() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_test_clip(tmp.path(), "input_extra.mp4", "640x360", 1, 24, "libx264");
    let output = tmp.path().join("extra_args.mp4");

    let job = EncodeJob {
        input: input.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X264,
        crf: 23,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        extra_args: vec!["-g".into(), "30".into()],
    };

    let result = encode(job, None).await.unwrap();
    assert!(result.file_size > 0);
    assert!(output.exists());
}

#[tokio::test]
async fn fate_encode_reference_then_distorted_quality_check() {
    if !has_ffmpeg() {
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let input = generate_reference_clip(tmp.path(), "ref_lossless.mp4", "640x360", 2);
    let output = tmp.path().join("distorted.mp4");

    let job = EncodeJob {
        input: input.to_string_lossy().into_owned(),
        output: output.to_string_lossy().into_owned(),
        resolution: None,
        codec: Codec::X264,
        crf: 30,
        rate_control: RateControlMode::Crf,
        target_bitrate: 0.0,
        max_bitrate: 0.0,
        bufsize: 0.0,
        preset: "ultrafast".into(),
        extra_args: vec![],
    };

    let result = encode(job, None).await.unwrap();

    // Verify the distorted file is smaller than lossless
    let ref_size = std::fs::metadata(&input).unwrap().len();
    assert!(
        result.file_size < ref_size,
        "lossy encode ({}) should be smaller than lossless ({})",
        result.file_size,
        ref_size
    );
}
