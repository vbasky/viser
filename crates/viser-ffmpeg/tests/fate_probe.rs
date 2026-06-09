mod common;

use common::{check_ffmpeg, generate_hdr_clip, generate_test_clip};
use viser_ffmpeg::{ProbeCache, ProbeEngine, probe};

#[tokio::test]
async fn fate_probe_resolution_and_codec() {
    check_ffmpeg().expect("ffmpeg required");

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_test_clip(tmp.path(), "test_720p.mp4", "1280x720", 2, 30, "libx264");

    let result = probe(&clip.to_string_lossy()).await.unwrap();

    let video = result.video_stream().expect("must have video stream");
    assert_eq!(video.width, 1280);
    assert_eq!(video.height, 720);
    assert_eq!(video.codec_name, "h264");
    assert!(video.fps() > 29.0 && video.fps() < 31.0, "fps = {}", video.fps());
}

#[tokio::test]
async fn fate_probe_duration() {
    check_ffmpeg().expect("ffmpeg required");

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_test_clip(tmp.path(), "dur_test.mp4", "320x240", 2, 25, "libx264");

    let result = probe(&clip.to_string_lossy()).await.unwrap();

    assert!(result.format.duration > 1.5, "duration too short: {}", result.format.duration);
    assert!(result.format.duration < 3.0, "duration too long: {}", result.format.duration);
}

#[tokio::test]
async fn fate_probe_file_size() {
    check_ffmpeg().expect("ffmpeg required");

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_test_clip(tmp.path(), "size_test.mp4", "640x360", 1, 24, "libx264");

    let result = probe(&clip.to_string_lossy()).await.unwrap();

    assert!(result.format.size > 0, "file size should be > 0");
    assert!(result.format.bit_rate > 0, "bitrate should be > 0");
}

#[tokio::test]
async fn fate_probe_audio_stream() {
    check_ffmpeg().expect("ffmpeg required");

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_test_clip(tmp.path(), "audio_test.mp4", "320x240", 2, 25, "libx264");

    let result = probe(&clip.to_string_lossy()).await.unwrap();

    let audio = result.audio_stream().expect("must have audio stream");
    assert_eq!(audio.codec_type, "audio");
}

#[tokio::test]
async fn fate_probe_format_name() {
    check_ffmpeg().expect("ffmpeg required");

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_test_clip(tmp.path(), "fmt_test.mp4", "320x240", 1, 24, "libx264");

    let result = probe(&clip.to_string_lossy()).await.unwrap();

    assert!(
        result.format.format_name.contains("mp4"),
        "expected mp4 in format_name, got: {}",
        result.format.format_name
    );
}

#[tokio::test]
async fn fate_probe_hdr_detection() {
    check_ffmpeg().expect("ffmpeg required");

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_hdr_clip(tmp.path());

    let result = probe(&clip.to_string_lossy()).await.unwrap();
    let video = result.video_stream().expect("must have video stream");
    assert!(video.is_hdr(), "HDR clip with PQ transfer and BT.2020 primaries must be detected");
}

#[tokio::test]
async fn fate_probe_sdr_not_hdr() {
    check_ffmpeg().expect("ffmpeg required");

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_test_clip(tmp.path(), "sdr_test.mp4", "1280x720", 2, 30, "libx264");

    let result = probe(&clip.to_string_lossy()).await.unwrap();

    let video = result.video_stream().expect("must have video stream");
    assert!(!video.is_hdr(), "SDR clip should NOT be detected as HDR");
}

#[tokio::test]
async fn fate_probe_cache_deduplication() {
    check_ffmpeg().expect("ffmpeg required");

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_test_clip(tmp.path(), "cache_test.mp4", "640x360", 1, 24, "libx264");

    let cache = ProbeCache::new();
    let r1 = cache.probe(&clip.to_string_lossy()).await.unwrap();
    let r2 = cache.probe(&clip.to_string_lossy()).await.unwrap();

    assert!(
        (r1.format.duration - r2.format.duration).abs() < 1e-9,
        "cached probe should return same duration"
    );
    assert_eq!(r1.streams.len(), r2.streams.len(), "cached probe should return same stream count");
}

#[tokio::test]
async fn fate_probe_probe_engine_default() {
    check_ffmpeg().expect("ffmpeg required");

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_test_clip(tmp.path(), "engine_test.mp4", "320x240", 1, 24, "libx264");

    let cache = ProbeCache::with_engine(ProbeEngine::Ffprobe);
    let result = cache.probe(&clip.to_string_lossy()).await.unwrap();
    assert!(result.video_stream().is_some());
}

#[tokio::test]
async fn fate_probe_multi_resolution() {
    check_ffmpeg().expect("ffmpeg required");

    for (label, width, height) in
        &[("360p", 640, 360), ("480p", 854, 480), ("720p", 1280, 720), ("1080p", 1920, 1080)]
    {
        let tmp = tempfile::tempdir().unwrap();
        let clip = generate_test_clip(
            tmp.path(),
            &format!("multi_{label}.mp4"),
            &format!("{width}x{height}"),
            1,
            24,
            "libx264",
        );

        let result = probe(&clip.to_string_lossy()).await.unwrap();
        let video = result.video_stream().unwrap();
        assert_eq!(video.width, *width, "{}: width mismatch", label);
        assert_eq!(video.height, *height, "{}: height mismatch", label);
    }
}

#[tokio::test]
async fn fate_probe_returns_error_for_missing_file() {
    check_ffmpeg().expect("ffmpeg required");

    let result = probe("/nonexistent/file_that_does_not_exist_12345.mp4").await;
    assert!(result.is_err(), "probing nonexistent file should error");
}

#[tokio::test]
async fn fate_probe_frame_rate_parsing() {
    check_ffmpeg().expect("ffmpeg required");

    let tmp = tempfile::tempdir().unwrap();
    let clip = generate_test_clip(tmp.path(), "fps24.mp4", "640x360", 1, 24, "libx264");

    let result = probe(&clip.to_string_lossy()).await.unwrap();
    let video = result.video_stream().unwrap();

    assert!(video.fps() > 23.0 && video.fps() < 25.0, "expected ~24fps, got {}", video.fps());
    assert!(!video.r_frame_rate.is_empty(), "r_frame_rate should not be empty");
    assert!(!video.avg_frame_rate.is_empty(), "avg_frame_rate should not be empty");
}
