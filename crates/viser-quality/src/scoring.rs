//! Filter-graph preparation for quality scoring across bit depths and HDR.

use serde::{Deserialize, Serialize};
use tracing::warn;
use viser_ffmpeg::{StreamInfo, bit_depth, yuv420p_for_depth};

/// How HDR content should be prepared before VMAF/PSNR scoring.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HdrScoringMode {
    /// Tonemap HDR to BT.709 SDR for HDR sources; preserve bit depth for 10-bit SDR.
    #[default]
    Auto,
    /// Always tonemap HDR sources to BT.709 SDR before scoring.
    Tonemap,
    /// Keep native pixel format; may produce unreliable scores with SDR VMAF models.
    Native,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ScoringPrep {
    Passthrough,
    HighBitDepth { pix_fmt: String },
    TonemapSdr,
}

/// Resolved scoring preparation for a reference/distorted pair.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoringPlan {
    prep: ScoringPrep,
    scoring_depth: u8,
}

impl ScoringPlan {
    /// PSNR peak for the scoring color space.
    pub fn psnr_peak(&self) -> f64 {
        viser_ffmpeg::psnr_peak(self.scoring_depth)
    }
}

/// Resolves how reference and distorted streams should be prepared for scoring.
pub fn resolve_scoring_plan(
    reference: &StreamInfo,
    distorted: &StreamInfo,
    mode: HdrScoringMode,
) -> ScoringPlan {
    let ref_depth = bit_depth(reference);
    let dist_depth = bit_depth(distorted);
    if ref_depth != dist_depth {
        warn!(
            reference_depth = ref_depth,
            distorted_depth = dist_depth,
            "reference and distorted bit depths differ; scores may be unreliable"
        );
    }

    let prep = match mode {
        HdrScoringMode::Tonemap if reference.is_hdr() => ScoringPrep::TonemapSdr,
        HdrScoringMode::Native => {
            if ref_depth > 8 {
                ScoringPrep::HighBitDepth { pix_fmt: scoring_pix_fmt(reference) }
            } else {
                ScoringPrep::Passthrough
            }
        }
        HdrScoringMode::Auto | HdrScoringMode::Tonemap => {
            if reference.is_hdr() {
                ScoringPrep::TonemapSdr
            } else if ref_depth > 8 {
                ScoringPrep::HighBitDepth { pix_fmt: scoring_pix_fmt(reference) }
            } else {
                ScoringPrep::Passthrough
            }
        }
    };

    let scoring_depth = match prep {
        ScoringPrep::TonemapSdr | ScoringPrep::Passthrough => 8,
        ScoringPrep::HighBitDepth { .. } => ref_depth,
    };

    ScoringPlan { prep, scoring_depth }
}

fn scoring_pix_fmt(stream: &StreamInfo) -> String {
    if !stream.pix_fmt.is_empty() {
        stream.pix_fmt.clone()
    } else {
        yuv420p_for_depth(bit_depth(stream)).to_string()
    }
}

fn tonemap_to_sdr_filter(stream: &StreamInfo) -> String {
    let transfer = if stream.color_transfer.is_empty() {
        "bt2020-10".to_string()
    } else {
        stream.color_transfer.clone()
    };
    let primaries = if stream.color_primaries.is_empty() {
        "bt2020".to_string()
    } else {
        stream.color_primaries.clone()
    };
    let matrix = if stream.color_space.is_empty() {
        "bt2020nc".to_string()
    } else {
        stream.color_space.clone()
    };

    format!(
        "zscale=transfer={transfer}:matrix={matrix}:primaries={primaries},\
         zscale=transfer=linear:npl=100,format=gbrpf32le,\
         zscale=primaries=bt709,tonemap=tonemap=hable:desat=0,\
         zscale=transfer=bt709:matrix=bt709:primaries=bt709,format=yuv420p"
    )
}

/// Builds a libvmaf filtergraph with format/HDR preparation on both inputs.
pub fn build_vmaf_filtergraph(
    reference: &StreamInfo,
    distorted: &StreamInfo,
    mode: HdrScoringMode,
    width: i32,
    height: i32,
    vmaf_opts: &str,
) -> String {
    let plan = resolve_scoring_plan(reference, distorted, mode);
    let dist_filters = prep_filters(distorted, &plan, Some((width, height)));
    let ref_filters = prep_filters(reference, &plan, None);
    format!("[0:v]{dist_filters}[dist];[1:v]{ref_filters}[ref];[dist][ref]libvmaf={vmaf_opts}")
}

/// Builds a two-input filtergraph prefix ending at `[dist]` and `[ref]` labels.
pub fn build_compare_filtergraph(
    reference: &StreamInfo,
    distorted: &StreamInfo,
    mode: HdrScoringMode,
    width: i32,
    height: i32,
) -> (String, ScoringPlan) {
    let plan = resolve_scoring_plan(reference, distorted, mode);
    let dist_filters = prep_filters(distorted, &plan, Some((width, height)));
    let ref_filters = prep_filters(reference, &plan, None);
    let graph = format!("[0:v]{dist_filters}[dist];[1:v]{ref_filters}[ref]");
    (graph, plan)
}

fn prep_filters(stream: &StreamInfo, plan: &ScoringPlan, scale: Option<(i32, i32)>) -> String {
    let mut parts = Vec::new();
    if let Some((w, h)) = scale {
        parts.push(format!("scale={w}:{h}:flags=bicubic"));
    }
    match &plan.prep {
        ScoringPrep::Passthrough => {}
        ScoringPrep::HighBitDepth { pix_fmt } => parts.push(format!("format={pix_fmt}")),
        ScoringPrep::TonemapSdr => parts.push(tonemap_to_sdr_filter(stream)),
    }
    if parts.is_empty() { "null".to_string() } else { parts.join(",") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(pix_fmt: &str, transfer: &str, primaries: &str) -> StreamInfo {
        StreamInfo {
            index: 0,
            codec_name: "hevc".into(),
            codec_long_name: String::new(),
            codec_type: "video".into(),
            profile: String::new(),
            width: 1920,
            height: 1080,
            pix_fmt: pix_fmt.into(),
            level: 0,
            field_order: String::new(),
            color_range: String::new(),
            color_space: if primaries == "bt709" { "bt709".into() } else { "bt2020nc".into() },
            color_transfer: transfer.into(),
            color_primaries: primaries.into(),
            duration: 0.0,
            bit_rate: 0,
            nb_frames: 0,
            r_frame_rate: "24/1".into(),
            avg_frame_rate: "24/1".into(),
            sample_rate: 0,
            channels: 0,
            channel_layout: String::new(),
            bits_per_raw_sample: if pix_fmt.contains("10") { 10 } else { 8 },
        }
    }

    #[test]
    fn test_auto_hdr_uses_tonemap() {
        let reference = stream("yuv420p10le", "smpte2084", "bt2020");
        let distorted = stream("yuv420p", "bt709", "bt709");
        let plan = resolve_scoring_plan(&reference, &distorted, HdrScoringMode::Auto);
        assert_eq!(plan.scoring_depth, 8);
        let graph = build_vmaf_filtergraph(
            &reference,
            &distorted,
            HdrScoringMode::Auto,
            1920,
            1080,
            "log_fmt=json",
        );
        assert!(graph.contains("tonemap"));
    }

    #[test]
    fn test_auto_10bit_sdr_preserves_depth() {
        let reference = stream("yuv420p10le", "bt709", "bt709");
        let distorted = stream("yuv420p10le", "bt709", "bt709");
        let plan = resolve_scoring_plan(&reference, &distorted, HdrScoringMode::Auto);
        assert_eq!(plan.scoring_depth, 10);
        let graph = build_vmaf_filtergraph(
            &reference,
            &distorted,
            HdrScoringMode::Auto,
            1920,
            1080,
            "log_fmt=json",
        );
        assert!(graph.contains("format=yuv420p10le"));
    }

    #[test]
    fn test_vmaf_filtergraph_includes_tonemap_for_hdr() {
        let reference = stream("yuv420p10le", "smpte2084", "bt2020");
        let distorted = stream("yuv420p", "bt709", "bt709");
        let graph = build_vmaf_filtergraph(
            &reference,
            &distorted,
            HdrScoringMode::Auto,
            1920,
            1080,
            "log_fmt=json",
        );
        assert!(graph.contains("tonemap"));
        assert!(graph.contains("libvmaf="));
    }
}
