//! Bit depth, pixel format, and HDR color metadata helpers (FFmpeg-specific).
//!
//! Engine-agnostic types and pure helpers live in `viser-engine` and are
//! re-exported from this crate's root.

use crate::{Codec, CodecFamily, SourceFormat, codec_supports_bit_depth, hw_pix_fmt};

/// Probes and attaches HDR10 static metadata when the source is HDR.
///
/// A no-op for SDR sources. Probe failures are swallowed (best-effort): a
/// missing mastering-display block degrades to colour-primary signalling
/// rather than failing the encode.
pub async fn enrich_hdr10(mut format: SourceFormat, path: &str) -> SourceFormat {
    if format.is_hdr {
        if let Ok(Some(md)) = crate::probe_hdr10_metadata(path).await {
            format.hdr10 = Some(md);
        }
    }
    format
}

/// FFmpeg output arguments that preserve source bit depth and HDR metadata.
pub fn encode_color_args(codec: Codec, format: &SourceFormat) -> Vec<String> {
    let mut args = Vec::new();

    if format.is_high_bit_depth() && codec_supports_bit_depth(codec, format.bit_depth) {
        if codec.is_software() {
            args.extend(["-pix_fmt".into(), format.pix_fmt.clone()]);
            match codec {
                Codec::X264 => args.extend(["-profile:v".into(), "high10".into()]),
                Codec::X265 => args.extend(["-x265-params".into(), x265_params(format)]),
                Codec::SvtAv1 => {}
                _ => {}
            }
        } else if let Some(pix) = hw_pix_fmt(format, codec) {
            args.extend(["-pix_fmt".into(), pix]);
            // Hardware HEVC and AV1 encoders need an explicit main10 profile.
            if matches!(codec.family(), CodecFamily::H265 | CodecFamily::Av1) {
                args.extend(["-profile:v".into(), "main10".into()]);
            }
        }
    }

    if format.is_hdr {
        append_color_metadata(&mut args, format);
        match codec {
            Codec::X265 => merge_x265_color_params(&mut args, format),
            Codec::SvtAv1 => {
                let params = svtav1_hdr_params(format);
                if !params.is_empty() {
                    // A second `-svtav1-params` may be added by the rate-control
                    // builder; `coalesce_svtav1_params` merges them before the
                    // encode runs (the last flag would otherwise win outright).
                    args.extend(["-svtav1-params".into(), params]);
                }
            }
            _ => add_hdr_bsf(&mut args, codec.family(), format),
        }
    }

    args
}

/// Injects HDR10 static metadata (mastering-display + max-cll) via a
/// codec-family bitstream filter. This works with ANY encoder (including
/// hardware) because it operates on the encoded bitstream after encoding,
/// before muxing.
///
/// Supported codec families:
/// - `H265` → `hevc_metadata` bitstream filter
/// - `H264` → `h264_metadata` bitstream filter
/// - `Av1` / `Vp9` → not yet supported (falls back to `-color_*` tags only)
fn add_hdr_bsf(args: &mut Vec<String>, family: CodecFamily, format: &SourceFormat) {
    let Some(hdr10) = &format.hdr10 else {
        return;
    };
    if hdr10.is_empty() {
        return;
    }

    let bsf_name = match family {
        CodecFamily::H265 => "hevc_metadata",
        CodecFamily::H264 => "h264_metadata",
        // av1_metadata does not support mastering_display/max_cll options;
        // VP9 has no equivalent bitstream filter.
        _ => return,
    };

    let mut params = Vec::new();
    if let Some(display) = &hdr10.mastering_display {
        params.push(format!("mastering_display=\"{}\"", display.to_x265_string()));
    }
    if let Some(max_cll) = hdr10.max_cll {
        let max_fall = hdr10.max_fall.unwrap_or(0);
        params.push(format!("max_cll={max_cll},{max_fall}"));
    }
    if !params.is_empty() {
        args.extend(["-bsf".into(), format!("{}={}", bsf_name, params.join(":"))]);
    }
}

fn append_color_metadata(args: &mut Vec<String>, format: &SourceFormat) {
    if !format.color_primaries.is_empty() {
        args.extend(["-color_primaries".into(), format.color_primaries.clone()]);
    }
    if !format.color_transfer.is_empty() {
        args.extend(["-color_trc".into(), format.color_transfer.clone()]);
    }
    if !format.color_space.is_empty() {
        args.extend(["-colorspace".into(), format.color_space.clone()]);
    }
}

fn x265_params(format: &SourceFormat) -> String {
    let mut parts = Vec::new();
    if format.bit_depth > 8 {
        parts.push("profile=main10".into());
    }
    if format.is_hdr {
        if !format.color_primaries.is_empty() {
            parts.push(format!("colorprim={}", format.color_primaries));
        }
        if !format.color_transfer.is_empty() {
            parts.push(format!("transfer={}", format.color_transfer));
        }
        if !format.color_space.is_empty() {
            parts.push(format!("colormatrix={}", format.color_space));
        }
        if let Some(hdr10) = &format.hdr10 {
            if let Some(display) = &hdr10.mastering_display {
                parts.push(format!("master-display={}", display.to_x265_string()));
            }
            if let Some(max_cll) = hdr10.max_cll {
                // x265 expects "MaxCLL,MaxFALL"; MaxFALL defaults to 0 when absent.
                parts.push(format!("max-cll={},{}", max_cll, hdr10.max_fall.unwrap_or(0)));
            }
        }
    }
    parts.join(":")
}

/// SVT-AV1 `-svtav1-params` HDR10 static-metadata fragment (without the flag).
///
/// SVT-AV1 shares x265's `mastering-display` grammar but spells content light
/// `content-light=MaxCLL,MaxFALL`. Colour primaries/transfer/matrix are carried
/// by the standard `-color_*` options [`append_color_metadata`] emits.
fn svtav1_hdr_params(format: &SourceFormat) -> String {
    let mut parts = Vec::new();
    if let Some(hdr10) = &format.hdr10 {
        if let Some(display) = &hdr10.mastering_display {
            parts.push(format!("mastering-display={}", display.to_svtav1_string()));
        }
        if let Some(max_cll) = hdr10.max_cll {
            parts.push(format!("content-light={},{}", max_cll, hdr10.max_fall.unwrap_or(0)));
        }
    }
    parts.join(":")
}

fn merge_x265_color_params(args: &mut Vec<String>, format: &SourceFormat) {
    let color = x265_params(format);
    if color.is_empty() {
        return;
    }
    if let Some(idx) = args.iter().position(|a| a == "-x265-params") {
        let existing = args.get(idx + 1).cloned().unwrap_or_default();
        let merged = if existing.is_empty() { color } else { format!("{existing}:{color}") };
        args[idx + 1] = merged;
    } else {
        args.extend(["-x265-params".into(), color]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SourceFormat, StreamInfo, bit_depth, psnr_peak};

    fn base_stream() -> StreamInfo {
        StreamInfo {
            index: 0,
            codec_name: "h264".into(),
            codec_long_name: String::new(),
            codec_type: "video".into(),
            profile: String::new(),
            width: 1920,
            height: 1080,
            pix_fmt: "yuv420p".into(),
            level: 0,
            field_order: String::new(),
            color_range: String::new(),
            color_space: "bt709".into(),
            color_transfer: "bt709".into(),
            color_primaries: "bt709".into(),
            duration: 0.0,
            bit_rate: 0,
            nb_frames: 0,
            r_frame_rate: "24/1".into(),
            avg_frame_rate: "24/1".into(),
            sample_rate: 0,
            channels: 0,
            channel_layout: String::new(),
            bits_per_raw_sample: 8,
        }
    }

    #[test]
    fn test_bit_depth_from_pix_fmt() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        assert_eq!(bit_depth(&stream), 10);
    }

    #[test]
    fn test_source_format_high_bit_depth() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        let format = SourceFormat::from_stream(&stream);
        assert_eq!(format.bit_depth, 10);
        assert_eq!(format.pix_fmt, "yuv420p10le");
        assert!(format.is_high_bit_depth());
    }

    #[test]
    fn test_encode_color_args_x265_10bit_hdr() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.color_transfer = "smpte2084".into();
        stream.color_primaries = "bt2020".into();
        stream.color_space = "bt2020nc".into();
        let format = SourceFormat::from_stream(&stream);
        let args = encode_color_args(Codec::X265, &format);
        assert!(args.windows(2).any(|w| w[0] == "-pix_fmt" && w[1] == "yuv420p10le"));
        assert!(args.iter().any(|a| a.contains("profile=main10")));
        assert!(args.iter().any(|a| a.contains("transfer=smpte2084")));
    }

    #[test]
    fn test_psnr_peak_scaling() {
        assert_eq!(psnr_peak(8), 255.0);
        assert_eq!(psnr_peak(10), 1023.0);
    }

    #[test]
    fn test_encode_color_args_emits_hdr10_metadata() {
        use crate::{Hdr10Metadata, MasteringDisplay};
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.color_transfer = "smpte2084".into();
        stream.color_primaries = "bt2020".into();
        stream.color_space = "bt2020nc".into();
        let mut format = SourceFormat::from_stream(&stream);
        format.hdr10 = Some(Hdr10Metadata {
            mastering_display: Some(MasteringDisplay {
                green_x: 13250,
                green_y: 34500,
                blue_x: 7500,
                blue_y: 3000,
                red_x: 34000,
                red_y: 16000,
                white_x: 15635,
                white_y: 16450,
                max_luminance: 10_000_000,
                min_luminance: 50,
            }),
            max_cll: Some(1000),
            max_fall: Some(400),
        });
        let args = encode_color_args(Codec::X265, &format);
        let params = args
            .windows(2)
            .find(|w| w[0] == "-x265-params")
            .map(|w| w[1].clone())
            .expect("x265-params present");
        assert!(
            params.contains("master-display=G(13250,34500)B(7500,3000)R(34000,16000)WP(15635,16450)L(10000000,50)"),
            "got: {params}"
        );
        assert!(params.contains("max-cll=1000,400"), "got: {params}");
    }

    #[test]
    fn test_encode_color_args_svtav1_hdr10_metadata() {
        use crate::{Hdr10Metadata, MasteringDisplay};
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.color_transfer = "smpte2084".into();
        stream.color_primaries = "bt2020".into();
        stream.color_space = "bt2020nc".into();
        let mut format = SourceFormat::from_stream(&stream);
        format.hdr10 = Some(Hdr10Metadata {
            mastering_display: Some(MasteringDisplay {
                green_x: 13250,
                green_y: 34500,
                blue_x: 7500,
                blue_y: 3000,
                red_x: 34000,
                red_y: 16000,
                white_x: 15635,
                white_y: 16450,
                max_luminance: 10_000_000,
                min_luminance: 50,
            }),
            max_cll: Some(1000),
            max_fall: Some(400),
        });
        let args = encode_color_args(Codec::SvtAv1, &format);
        let params = args
            .windows(2)
            .find(|w| w[0] == "-svtav1-params")
            .map(|w| w[1].clone())
            .expect("svtav1-params present");
        assert!(
            params.contains("mastering-display=G(0.265,0.69)B(0.15,0.06)R(0.68,0.32)WP(0.3127,0.329)L(1000,0.005)"),
            "got: {params}"
        );
        assert!(params.contains("content-light=1000,400"), "got: {params}");
        // AV1 carries colour primaries/transfer via the standard -color_* options.
        assert!(args.windows(2).any(|w| w[0] == "-color_trc" && w[1] == "smpte2084"));
    }

    #[test]
    fn test_encode_color_args_no_hdr10_when_sdr() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        let format = SourceFormat::from_stream(&stream);
        let args = encode_color_args(Codec::X265, &format);
        assert!(!args.iter().any(|a| a.contains("master-display")));
        assert!(!args.iter().any(|a| a.contains("max-cll")));
    }

    // ── hw_pix_fmt ──

    #[test]
    fn test_hw_pix_fmt_nvenc_10bit() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        let format = SourceFormat::from_stream(&stream);
        assert_eq!(hw_pix_fmt(&format, Codec::NvencH265), Some("p010le".into()));
        assert_eq!(hw_pix_fmt(&format, Codec::NvencAv1), Some("p010le".into()));
    }

    #[test]
    fn test_hw_pix_fmt_8bit_shows_none() {
        let format = SourceFormat::from_stream(&base_stream());
        assert_eq!(hw_pix_fmt(&format, Codec::NvencH265), None);
        assert_eq!(hw_pix_fmt(&format, Codec::QsvH265), None);
        assert_eq!(hw_pix_fmt(&format, Codec::VaapiH265), None);
    }

    #[test]
    fn test_hw_pix_fmt_vaapi_returns_none() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        let format = SourceFormat::from_stream(&stream);
        assert_eq!(hw_pix_fmt(&format, Codec::VaapiH265), None);
    }

    #[test]
    fn test_hw_pix_fmt_amf_10bit() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        let format = SourceFormat::from_stream(&stream);
        assert_eq!(hw_pix_fmt(&format, Codec::AmfH265), Some("p010le".into()));
    }

    #[test]
    fn test_hw_pix_fmt_qsv_10bit() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        let format = SourceFormat::from_stream(&stream);
        assert_eq!(hw_pix_fmt(&format, Codec::QsvH265), Some("p010le".into()));
    }

    #[test]
    fn test_hw_pix_fmt_videotoolbox_10bit() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        let format = SourceFormat::from_stream(&stream);
        assert_eq!(hw_pix_fmt(&format, Codec::VideoToolboxH265), Some("p010le".into()));
    }

    // ── HW encoder high bit depth in encode_color_args ──

    #[test]
    fn test_encode_color_args_nvenc_high_bit_depth_pix_fmt() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        let format = SourceFormat::from_stream(&stream);
        let args = encode_color_args(Codec::NvencH265, &format);
        assert!(
            args.windows(2).any(|w| w[0] == "-pix_fmt" && w[1] == "p010le"),
            "expected -pix_fmt p010le for NVENC 10-bit, got {args:?}"
        );
    }

    #[test]
    fn test_encode_color_args_nvenc_hdr10_sets_color_and_bsf() {
        use crate::{Hdr10Metadata, MasteringDisplay};
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        stream.color_transfer = "smpte2084".into();
        stream.color_primaries = "bt2020".into();
        stream.color_space = "bt2020nc".into();
        let mut format = SourceFormat::from_stream(&stream);
        format.hdr10 = Some(Hdr10Metadata {
            mastering_display: Some(MasteringDisplay {
                green_x: 13250,
                green_y: 34500,
                blue_x: 7500,
                blue_y: 3000,
                red_x: 34000,
                red_y: 16000,
                white_x: 15635,
                white_y: 16450,
                max_luminance: 10_000_000,
                min_luminance: 50,
            }),
            max_cll: Some(1000),
            max_fall: Some(400),
        });
        let args = encode_color_args(Codec::NvencH265, &format);
        // Must set the HDR color tags.
        assert!(args.windows(2).any(|w| w[0] == "-color_trc" && w[1] == "smpte2084"));
        // Must set the pixel format for 10-bit.
        assert!(args.windows(2).any(|w| w[0] == "-pix_fmt" && w[1] == "p010le"));
        // Must inject HDR metadata via hevc_metadata bitstream filter.
        let bsf_idx = args.iter().position(|a| a == "-bsf").expect("missing -bsf");
        let bsf_val = &args[bsf_idx + 1];
        assert!(
            bsf_val.starts_with("hevc_metadata="),
            "expected hevc_metadata BSF, got: {bsf_val}"
        );
        assert!(bsf_val.contains("mastering_display="));
        assert!(bsf_val.contains("max_cll=1000,400"));
    }

    #[test]
    fn test_encode_color_args_amf_high_bit_depth_pix_fmt() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        let format = SourceFormat::from_stream(&stream);
        let args = encode_color_args(Codec::AmfH265, &format);
        assert!(
            args.windows(2).any(|w| w[0] == "-pix_fmt" && w[1] == "p010le"),
            "expected -pix_fmt p010le for AMF 10-bit, got {args:?}"
        );
    }

    #[test]
    fn test_encode_color_args_qsv_high_bit_depth_pix_fmt() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        let format = SourceFormat::from_stream(&stream);
        let args = encode_color_args(Codec::QsvH265, &format);
        assert!(
            args.windows(2).any(|w| w[0] == "-pix_fmt" && w[1] == "p010le"),
            "expected -pix_fmt p010le for QSV 10-bit, got {args:?}"
        );
    }

    #[test]
    fn test_encode_color_args_videotoolbox_high_bit_depth_pix_fmt() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        let format = SourceFormat::from_stream(&stream);
        let args = encode_color_args(Codec::VideoToolboxH265, &format);
        assert!(
            args.windows(2).any(|w| w[0] == "-pix_fmt" && w[1] == "p010le"),
            "expected -pix_fmt p010le for VideoToolbox 10-bit, got {args:?}"
        );
    }

    #[test]
    fn test_encode_color_args_nvenc_h264_8bit_does_not_set_p010() {
        // H.264 NVENC is 8-bit only; must not set p010le.
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        let format = SourceFormat::from_stream(&stream);
        let args = encode_color_args(Codec::NvencH264, &format);
        assert!(!args.windows(2).any(|w| w == ["-pix_fmt", "p010le"]));
        assert!(args.is_empty(), "expected no color args for H.264 NVENC 10-bit: {args:?}");
    }

    #[test]
    fn test_encode_color_args_adds_profile_for_hw_hevc_10bit() {
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        let format = SourceFormat::from_stream(&stream);
        for codec in &[Codec::NvencH265, Codec::QsvH265, Codec::AmfH265, Codec::VideoToolboxH265] {
            let args = encode_color_args(*codec, &format);
            assert!(
                args.windows(2).any(|w| w == ["-profile:v", "main10"]),
                "{codec:?}: expected -profile:v main10, got {args:?}"
            );
        }
    }

    #[test]
    fn test_encode_color_args_svtav1_hdr10_uses_svtav1_params_not_bsf() {
        use crate::{Hdr10Metadata, MasteringDisplay};
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        stream.color_transfer = "smpte2084".into();
        stream.color_primaries = "bt2020".into();
        stream.color_space = "bt2020nc".into();
        let mut format = SourceFormat::from_stream(&stream);
        format.hdr10 = Some(Hdr10Metadata {
            mastering_display: Some(MasteringDisplay {
                green_x: 13250,
                green_y: 34500,
                blue_x: 7500,
                blue_y: 3000,
                red_x: 34000,
                red_y: 16000,
                white_x: 15635,
                white_y: 16450,
                max_luminance: 10_000_000,
                min_luminance: 50,
            }),
            max_cll: Some(1000),
            max_fall: Some(400),
        });
        let args = encode_color_args(Codec::SvtAv1, &format);
        // SVT-AV1 must NOT get a bitstream filter; it uses -svtav1-params instead.
        assert!(!args.iter().any(|a| a == "-bsf"), "SVT-AV1 should not use BSF: {args:?}");
        assert!(
            args.windows(2).any(|w| w[0] == "-svtav1-params"),
            "SVT-AV1 should use -svtav1-params: {args:?}"
        );
    }

    #[test]
    fn test_encode_color_args_x265_hdr10_uses_x265_params_not_bsf() {
        use crate::{Hdr10Metadata, MasteringDisplay};
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        stream.color_transfer = "smpte2084".into();
        stream.color_primaries = "bt2020".into();
        stream.color_space = "bt2020nc".into();
        let mut format = SourceFormat::from_stream(&stream);
        format.hdr10 = Some(Hdr10Metadata {
            mastering_display: Some(MasteringDisplay {
                green_x: 13250,
                green_y: 34500,
                blue_x: 7500,
                blue_y: 3000,
                red_x: 34000,
                red_y: 16000,
                white_x: 15635,
                white_y: 16450,
                max_luminance: 10_000_000,
                min_luminance: 50,
            }),
            max_cll: Some(1000),
            max_fall: Some(400),
        });
        let args = encode_color_args(Codec::X265, &format);
        assert!(!args.iter().any(|a| a == "-bsf"), "x265 should not use BSF: {args:?}");
        assert!(
            args.windows(2).any(|w| w[0] == "-x265-params"),
            "x265 should use -x265-params: {args:?}"
        );
    }

    #[test]
    fn test_encode_color_args_nvenc_av1_no_bsf() {
        // AV1 hardware encoders don't get a bitstream filter because
        // av1_metadata does not support mastering_display/max_cll.
        let mut stream = base_stream();
        stream.pix_fmt = "yuv420p10le".into();
        stream.bits_per_raw_sample = 10;
        stream.color_transfer = "smpte2084".into();
        stream.color_primaries = "bt2020".into();
        stream.color_space = "bt2020nc".into();
        let format = SourceFormat::from_stream(&stream);
        let args = encode_color_args(Codec::NvencAv1, &format);
        assert!(!args.iter().any(|a| a == "-bsf"), "AV1 should not use BSF: {args:?}");
        // Color tags must still be present.
        assert!(args.windows(2).any(|w| w[0] == "-color_trc" && w[1] == "smpte2084"));
    }

    // ── codec_supports_bit_depth ──

    #[test]
    fn test_codec_supports_bit_depth_10bit_hw() {
        assert!(codec_supports_bit_depth(Codec::NvencH265, 10));
        assert!(codec_supports_bit_depth(Codec::QsvH265, 10));
        assert!(codec_supports_bit_depth(Codec::AmfH265, 10));
        assert!(codec_supports_bit_depth(Codec::VideoToolboxH265, 10));
        assert!(codec_supports_bit_depth(Codec::VaapiH265, 10));
        assert!(codec_supports_bit_depth(Codec::NvencAv1, 10));
        assert!(codec_supports_bit_depth(Codec::QsvAv1, 10));
        assert!(codec_supports_bit_depth(Codec::AmfAv1, 10));
        assert!(codec_supports_bit_depth(Codec::VaapiAv1, 10));
    }

    #[test]
    fn test_codec_supports_bit_depth_h264_hw_8bit_only() {
        assert!(!codec_supports_bit_depth(Codec::NvencH264, 10));
        assert!(!codec_supports_bit_depth(Codec::QsvH264, 10));
        assert!(!codec_supports_bit_depth(Codec::AmfH264, 10));
        assert!(!codec_supports_bit_depth(Codec::VideoToolboxH264, 10));
        assert!(!codec_supports_bit_depth(Codec::VaapiH264, 10));
    }
}
