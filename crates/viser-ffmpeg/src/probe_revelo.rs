use crate::{FormatInfo, ProbeResult, StreamInfo};

/// Runs revelo's pure-Rust parser on the given file and returns parsed results
/// in viser's ffprobe-compatible `ProbeResult` format.
pub async fn probe(path: &str) -> anyhow::Result<ProbeResult> {
    let bytes = std::fs::read(path).map_err(|e| anyhow::anyhow!("failed to read {path}: {e}"))?;
    probe_bytes(&bytes, path)
}

/// Runs revelo on an in-memory buffer (for ProbeCache reuse).
pub(crate) fn probe_bytes(bytes: &[u8], filename: &str) -> anyhow::Result<ProbeResult> {
    let meta = revelo::Metadata::from_bytes(bytes)
        .ok_or_else(|| anyhow::anyhow!("revelo: no parser matched {filename}"))?;

    let general: Vec<(String, String)> =
        meta.general().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let vid: Vec<(String, String)> =
        meta.video().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let aud: Vec<(String, String)> =
        meta.audio().map(|(k, v)| (k.to_string(), v.to_string())).collect();

    let format = FormatInfo {
        filename: filename.to_string(),
        format_name: find_str(&general, "Format").unwrap_or("").to_string(),
        format_long_name: find_str(&general, "Format_Info")
            .or_else(|| find_str(&general, "Format_Commercial"))
            .unwrap_or("")
            .to_string(),
        duration: parse_duration_ms(&general),
        size: find_str(&general, "FileSize").and_then(|s| s.parse().ok()).unwrap_or(0),
        bit_rate: find_str(&general, "OverallBitRate")
            .and_then(|s| s.parse::<f64>().ok())
            .map(|b| (b * 1000.0) as i64)
            .unwrap_or(
                find_str(&general, "OverallBitRate_Nominal")
                    .and_then(|s| s.parse::<f64>().ok())
                    .map(|b| (b * 1000.0) as i64)
                    .unwrap_or(0),
            ),
        probe_score: 100,
    };

    let mut streams = Vec::new();
    let mut idx = 0;

    if !vid.is_empty() {
        streams.push(build_video_stream(idx, &vid));
        idx += 1;
    }
    if !aud.is_empty() {
        streams.push(build_audio_stream(idx, &aud));
    }

    Ok(ProbeResult { format, streams })
}

fn build_video_stream(index: i32, fields: &[(String, String)]) -> StreamInfo {
    let codec_name = find_str(fields, "CodecID").unwrap_or("").to_string();
    let width = find_str(fields, "Width").and_then(|s| s.parse().ok()).unwrap_or(0);
    let height = find_str(fields, "Height").and_then(|s| s.parse().ok()).unwrap_or(0);
    let fps = fps_from_fields(fields);

    StreamInfo {
        index,
        codec_name: codec_to_ffmpeg(&codec_name),
        codec_long_name: find_str(fields, "Codec_Info")
            .or_else(|| find_str(fields, "Format_Info"))
            .unwrap_or("")
            .to_string(),
        codec_type: "video".into(),
        profile: find_str(fields, "Format_Profile").unwrap_or("").to_string(),
        width,
        height,
        pix_fmt: pix_fmt_from_fields(fields),
        level: find_str(fields, "Format_Level")
            .and_then(|s| s.parse::<f64>().ok())
            .map(|l| l as i32)
            .unwrap_or(0),
        field_order: find_str(fields, "ScanType")
            .map(|s| match s {
                "Interlaced" | "MBAFF" => "tt".to_string(),
                _ => "progressive".to_string(),
            })
            .unwrap_or_else(|| "progressive".into()),
        color_range: find_str(fields, "colour_range").unwrap_or("").to_string(),
        color_space: find_str(fields, "ColorSpace").unwrap_or("").to_string(),
        color_transfer: transfer_to_ffmpeg(
            find_str(fields, "transfer_characteristics").unwrap_or(""),
        ),
        color_primaries: primaries_to_ffmpeg(find_str(fields, "colour_primaries").unwrap_or("")),
        duration: parse_duration_ms(fields),
        bit_rate: find_str(fields, "BitRate")
            .and_then(|s| s.parse::<f64>().ok())
            .map(|b| (b * 1000.0) as i64)
            .unwrap_or(0),
        nb_frames: find_str(fields, "FrameCount").and_then(|s| s.parse().ok()).unwrap_or(0),
        r_frame_rate: fps.clone(),
        avg_frame_rate: fps,
        sample_rate: 0,
        channels: 0,
        channel_layout: String::new(),
        bits_per_raw_sample: find_str(fields, "BitDepth").and_then(|s| s.parse().ok()).unwrap_or(0),
    }
}

fn build_audio_stream(index: i32, fields: &[(String, String)]) -> StreamInfo {
    let codec_name = find_str(fields, "CodecID").unwrap_or("").to_string();

    StreamInfo {
        index,
        codec_name: codec_to_ffmpeg(&codec_name),
        codec_long_name: find_str(fields, "Codec_Info").unwrap_or("").to_string(),
        codec_type: "audio".into(),
        profile: String::new(),
        width: 0,
        height: 0,
        pix_fmt: String::new(),
        level: 0,
        field_order: String::new(),
        color_range: String::new(),
        color_space: String::new(),
        color_transfer: String::new(),
        color_primaries: String::new(),
        duration: parse_duration_ms(fields),
        bit_rate: find_str(fields, "BitRate")
            .and_then(|s| s.parse::<f64>().ok())
            .map(|b| (b * 1000.0) as i64)
            .unwrap_or(0),
        nb_frames: 0,
        r_frame_rate: String::new(),
        avg_frame_rate: String::new(),
        sample_rate: find_str(fields, "SamplingRate").and_then(|s| s.parse().ok()).unwrap_or(0),
        channels: find_str(fields, "Channels").and_then(|s| s.parse().ok()).unwrap_or(0),
        channel_layout: find_str(fields, "ChannelLayout")
            .or_else(|| find_str(fields, "ChannelPositions"))
            .unwrap_or("")
            .to_string(),
        bits_per_raw_sample: find_str(fields, "BitDepth").and_then(|s| s.parse().ok()).unwrap_or(0),
    }
}

// --- helpers ---

fn find_str<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn parse_duration_ms(fields: &[(String, String)]) -> f64 {
    find_str(fields, "Duration")
        .and_then(|s| s.parse::<f64>().ok())
        .map(|ms| ms / 1000.0)
        .unwrap_or(0.0)
}

fn fps_from_fields(fields: &[(String, String)]) -> String {
    find_str(fields, "FrameRate")
        .and_then(|s| s.parse::<f64>().ok())
        .map(format_fps)
        .unwrap_or_else(String::new)
}

fn format_fps(fps: f64) -> String {
    let common = [
        (24000.0 / 1001.0, "24000/1001"),
        (30000.0 / 1001.0, "30000/1001"),
        (60000.0 / 1001.0, "60000/1001"),
    ];
    for (val, repr) in &common {
        if (fps - val).abs() < 0.01 {
            return repr.to_string();
        }
    }
    format!("{}/1", fps.round() as i32)
}

fn codec_to_ffmpeg(revelo_codec: &str) -> String {
    let lower = revelo_codec.to_lowercase();
    if lower.contains("avc") || lower.contains("264") {
        return "h264".into();
    }
    if lower.contains("hevc")
        || lower.contains("265")
        || lower.contains("hvc")
        || lower.contains("hev")
    {
        return "hevc".into();
    }
    if lower.contains("av1") || lower.contains("av01") {
        return "av1".into();
    }
    if lower.contains("vp9") {
        return "vp9".into();
    }
    if lower.contains("vp8") {
        return "vp8".into();
    }
    if lower.contains("mpeg audio") && lower.contains("layer 3") {
        return "mp3".into();
    }
    if lower.contains("aac") || lower.contains("mp4a") {
        return "aac".into();
    }
    if lower.contains("ac-3") || lower.contains("ac3") || lower.contains("e-ac-3") {
        return "ac3".into();
    }
    if lower.contains("dts") {
        return "dts".into();
    }
    if lower.contains("flac") {
        return "flac".into();
    }
    if lower.contains("opus") {
        return "opus".into();
    }
    if lower.contains("vorbis") {
        return "vorbis".into();
    }
    if lower.contains("pcm") {
        return "pcm_s16le".into();
    }
    if lower.contains("truehd") {
        return "truehd".into();
    }
    if lower.contains("dolby e") {
        return "dolby_e".into();
    }
    if lower.contains("prores") {
        return "prores".into();
    }
    if lower.contains("vc-3") || lower.contains("dnx") {
        return "dnxhd".into();
    }
    if lower.contains("vc-1") {
        return "vc1".into();
    }
    if lower.contains("mpeg-4v") || lower.contains("mpeg-4 visual") {
        return "mpeg4".into();
    }
    if lower.contains("mpeg-2") {
        return "mpeg2video".into();
    }
    if lower.contains("mpeg-1") {
        return "mpeg1video".into();
    }
    if lower.contains("theora") {
        return "theora".into();
    }
    if lower.contains("h.263") {
        return "h263".into();
    }
    lower
}

fn transfer_to_ffmpeg(revelo_transfer: &str) -> String {
    match revelo_transfer {
        "PQ" | "SMPTE ST 2084" => "smpte2084".into(),
        "HLG" | "ARIB STD-B67" => "arib-std-b67".into(),
        "BT.709" => "bt709".into(),
        "BT.2020 (10-bit)" | "BT.2020 (12-bit)" | "BT.2020" => "bt2020-10".into(),
        "SMPTE ST 428-1" => "smpte428".into(),
        s if !s.is_empty() => s.to_lowercase().replace(' ', "").replace('-', ""),
        _ => String::new(),
    }
}

fn primaries_to_ffmpeg(revelo_primaries: &str) -> String {
    match revelo_primaries {
        "BT.709" => "bt709".into(),
        "BT.2020" => "bt2020".into(),
        "BT.470-6 M" | "NTSC" => "bt470m".into(),
        "BT.470-6 B/G" | "PAL" => "bt470bg".into(),
        "BT.601 NTSC" | "SMPTE 170M" => "smpte170m".into(),
        "BT.601 PAL" | "SMPTE 240M" => "smpte240m".into(),
        "DCI P3" | "SMPTE RP 431-2" => "smpte431".into(),
        "Display P3" | "SMPTE RP 432-1" => "smpte432".into(),
        s if !s.is_empty() => s.to_lowercase().replace(' ', "").replace('-', ""),
        _ => String::new(),
    }
}

fn pix_fmt_from_fields(fields: &[(String, String)]) -> String {
    let chroma = find_str(fields, "ChromaSubsampling").unwrap_or("");
    let depth: i32 = find_str(fields, "BitDepth").and_then(|s| s.parse().ok()).unwrap_or(8);
    let has_alpha = find_str(fields, "Format_Settings").unwrap_or("").contains("alpha");

    match (chroma, depth, has_alpha) {
        ("4:2:0", 8, false) => "yuv420p".into(),
        ("4:2:0", 10, false) => "yuv420p10le".into(),
        ("4:2:0", 12, false) => "yuv420p12le".into(),
        ("4:2:2", 8, false) => "yuv422p".into(),
        ("4:2:2", 10, false) => "yuv422p10le".into(),
        ("4:4:4", 8, false) => "yuv444p".into(),
        ("4:4:4", 10, false) => "yuv444p10le".into(),
        ("4:4:4", 12, false) => "yuv444p12le".into(),
        ("4:4:4", 10, true) => "yuva444p10le".into(),
        // 8-bit formats carry no bit-depth suffix or endianness (e.g. `yuv420p`,
        // not `yuv420p8le`, which is not a valid FFmpeg pixel format).
        (_, d, _) if d <= 8 => "yuv420p".into(),
        _ => format!("yuv420p{depth}le"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_str() {
        let fields = vec![
            ("Format".to_string(), "MPEG-4".to_string()),
            ("Duration".to_string(), "5000.000".to_string()),
        ];
        assert_eq!(find_str(&fields, "Format"), Some("MPEG-4"));
        assert_eq!(find_str(&fields, "Missing"), None);
    }

    #[test]
    fn test_parse_duration_ms() {
        let fields = vec![("Duration".to_string(), "5000.000".to_string())];
        assert!((parse_duration_ms(&fields) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn test_format_fps() {
        assert_eq!(format_fps(23.976), "24000/1001");
        assert_eq!(format_fps(29.97), "30000/1001");
        assert_eq!(format_fps(25.0), "25/1");
        assert_eq!(format_fps(24.0), "24/1");
        assert_eq!(format_fps(50.0), "50/1");
    }

    #[test]
    fn test_codec_to_ffmpeg() {
        assert_eq!(codec_to_ffmpeg("avc1"), "h264");
        assert_eq!(codec_to_ffmpeg("hvc1"), "hevc");
        assert_eq!(codec_to_ffmpeg("av01"), "av1");
        assert_eq!(codec_to_ffmpeg("AAC LC"), "aac");
        assert_eq!(codec_to_ffmpeg("mp4a-40-2"), "aac");
        assert_eq!(codec_to_ffmpeg("Opus"), "opus");
        assert_eq!(codec_to_ffmpeg("V_VP9"), "vp9");
        assert_eq!(codec_to_ffmpeg("unknown_codec"), "unknown_codec");
    }

    #[test]
    fn test_transfer_to_ffmpeg() {
        assert_eq!(transfer_to_ffmpeg("PQ"), "smpte2084");
        assert_eq!(transfer_to_ffmpeg("HLG"), "arib-std-b67");
        assert_eq!(transfer_to_ffmpeg("BT.709"), "bt709");
        assert_eq!(transfer_to_ffmpeg("BT.2020 (10-bit)"), "bt2020-10");
        assert_eq!(transfer_to_ffmpeg(""), "");
    }

    #[test]
    fn test_primaries_to_ffmpeg() {
        assert_eq!(primaries_to_ffmpeg("BT.709"), "bt709");
        assert_eq!(primaries_to_ffmpeg("BT.2020"), "bt2020");
        assert_eq!(primaries_to_ffmpeg("DCI P3"), "smpte431");
        assert_eq!(primaries_to_ffmpeg(""), "");
    }

    #[test]
    fn test_pix_fmt_from_fields() {
        let fields: Vec<(String, String)> =
            vec![("ChromaSubsampling".into(), "4:2:0".into()), ("BitDepth".into(), "8".into())];
        assert_eq!(pix_fmt_from_fields(&fields), "yuv420p");

        let fields10: Vec<(String, String)> =
            vec![("ChromaSubsampling".into(), "4:2:0".into()), ("BitDepth".into(), "10".into())];
        assert_eq!(pix_fmt_from_fields(&fields10), "yuv420p10le");

        let fields422: Vec<(String, String)> =
            vec![("ChromaSubsampling".into(), "4:2:2".into()), ("BitDepth".into(), "10".into())];
        assert_eq!(pix_fmt_from_fields(&fields422), "yuv422p10le");

        // Unknown chroma at 8-bit falls back to plain `yuv420p`, not the
        // invalid `yuv420p8le`.
        let fields_no_chroma: Vec<(String, String)> = vec![("BitDepth".into(), "8".into())];
        assert_eq!(pix_fmt_from_fields(&fields_no_chroma), "yuv420p");
    }
}
