use crate::{Codec, ffmpeg_path};
use std::collections::HashSet;
use std::sync::OnceLock;
use tokio::process::Command;

static HW_ENCODERS_CACHE: OnceLock<HashSet<Codec>> = OnceLock::new();

static HW_ENCODERS_SEARCH: &[(Codec, &str)] = &[
    (Codec::NvencH264, "h264_nvenc"),
    (Codec::NvencH265, "hevc_nvenc"),
    (Codec::QsvH264, "h264_qsv"),
    (Codec::QsvH265, "hevc_qsv"),
    (Codec::VideoToolboxH264, "h264_videotoolbox"),
    (Codec::VideoToolboxH265, "hevc_videotoolbox"),
    (Codec::VaapiH264, "h264_vaapi"),
    (Codec::VaapiH265, "hevc_vaapi"),
    (Codec::AmfH264, "h264_amf"),
    (Codec::AmfH265, "hevc_amf"),
    (Codec::NvencAv1, "av1_nvenc"),
    (Codec::QsvAv1, "av1_qsv"),
    (Codec::VaapiAv1, "av1_vaapi"),
    (Codec::AmfAv1, "av1_amf"),
];

async fn probe_available_encoders() -> HashSet<String> {
    let result = Command::new(ffmpeg_path()).args(["-encoders"]).output().await;

    let Ok(output) = result else {
        return HashSet::new();
    };
    if !output.status.success() {
        return HashSet::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut encoders = HashSet::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if (trimmed.starts_with("V") || trimmed.starts_with("VE"))
            && let Some(name) = trimmed.split_whitespace().nth(1)
        {
            encoders.insert(name.to_string());
        }
    }
    encoders
}

async fn detect_hw_encoders_inner() -> HashSet<Codec> {
    let available = probe_available_encoders().await;
    HW_ENCODERS_SEARCH
        .iter()
        .filter_map(|(codec, name)| if available.contains(*name) { Some(*codec) } else { None })
        .collect()
}

pub async fn init_hw_encoders() {
    if HW_ENCODERS_CACHE.get().is_some() {
        return;
    }
    let codecs = detect_hw_encoders_inner().await;
    let _ = HW_ENCODERS_CACHE.set(codecs);
}

pub fn is_hw_encoder_available(codec: Codec) -> bool {
    assert!(codec.is_hardware(), "is_hw_encoder_available called on non-HW codec");
    HW_ENCODERS_CACHE.get().map(|set| set.contains(&codec)).unwrap_or(false)
}

pub fn hw_encoders_available() -> Vec<Codec> {
    HW_ENCODERS_CACHE
        .get()
        .map(|set| {
            let mut v: Vec<_> = set.iter().copied().collect();
            v.sort_by_key(|c| c.as_str());
            v
        })
        .unwrap_or_default()
}

pub fn list_hw_encoder_names() -> Vec<&'static str> {
    HW_ENCODERS_SEARCH.iter().map(|(_, name)| *name).collect()
}

// ── Hardware decode detection ──

static HW_DECODERS_CACHE: OnceLock<HashSet<String>> = OnceLock::new();

/// Hardware-accelerated decode methods viser knows how to drive, in preference
/// order. These map to FFmpeg `-hwaccel <name>` values.
static HW_DECODE_METHODS: &[&str] =
    &["cuda", "qsv", "vaapi", "videotoolbox", "d3d11va", "dxva2", "vdpau"];

async fn probe_available_hwaccels() -> HashSet<String> {
    let result = Command::new(ffmpeg_path()).args(["-hide_banner", "-hwaccels"]).output().await;

    let Ok(output) = result else {
        return HashSet::new();
    };
    if !output.status.success() {
        return HashSet::new();
    }

    // Output is a header line ("Hardware acceleration methods:") followed by one
    // method name per line.
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.contains(char::is_whitespace))
        .map(str::to_string)
        .collect()
}

/// Probes `ffmpeg -hwaccels` once and caches the available decode methods.
pub async fn init_hw_decoders() {
    if HW_DECODERS_CACHE.get().is_some() {
        return;
    }
    let available = probe_available_hwaccels().await;
    let known: HashSet<String> = HW_DECODE_METHODS
        .iter()
        .filter(|m| available.contains(**m))
        .map(|m| m.to_string())
        .collect();
    let _ = HW_DECODERS_CACHE.set(known);
}

/// Returns true when the given `-hwaccel` decode method is available.
pub fn is_hw_decoder_available(method: &str) -> bool {
    HW_DECODERS_CACHE.get().map(|set| set.contains(method)).unwrap_or(false)
}

/// Available hardware decode methods, sorted, after [`init_hw_decoders`].
pub fn hw_decoders_available() -> Vec<String> {
    HW_DECODERS_CACHE
        .get()
        .map(|set| {
            let mut v: Vec<_> = set.iter().cloned().collect();
            v.sort();
            v
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hw_encoders_cache_empty_before_init() {
        assert!(HW_ENCODERS_CACHE.get().is_none());
    }

    #[test]
    fn test_hw_encoder_search_has_expected_count() {
        assert_eq!(HW_ENCODERS_SEARCH.len(), 14);
    }

    #[test]
    fn test_search_all_have_matching_as_str() {
        for (codec, name) in HW_ENCODERS_SEARCH {
            assert_eq!(codec.as_str(), *name);
        }
    }
}
