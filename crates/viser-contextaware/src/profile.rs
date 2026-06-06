use serde::{Deserialize, Serialize};
use viser_ffmpeg::{
    Codec, RES_360P, RES_480P, RES_720P, RES_1080P, RES_1440P, RES_2160P, Resolution,
};
use viser_ladder::Opts as LadderOpts;

/// Target device class that determines resolution caps and codec preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    /// Smartphones and small tablets.
    Mobile,
    /// Laptops and desktop monitors.
    Desktop,
    /// 1080p TVs and large displays.
    Tv,
    /// 4K (2160p) TVs.
    Tv4k,
}

/// Encoding constraints and quality targets for a device class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Human-readable profile name (e.g. `"Mobile"`).
    pub name: String,
    /// Device class this profile targets.
    pub device: DeviceClass,
    /// Description of the intended device and viewing context.
    pub description: String,
    /// VMAF model name used for quality scoring (e.g. `"vmaf_v0.6.1"`).
    pub vmaf_model: String,
    /// Maximum resolution allowed for this device.
    pub max_res: Resolution,
    /// Candidate resolutions to evaluate.
    pub resolutions: Vec<Resolution>,
    /// Codecs to encode with, in preference order.
    pub codecs: Vec<Codec>,
    /// Ladder selection options (rung count, bitrate and VMAF bounds).
    pub ladder_opts: LadderOpts,
}

/// Returns the standard Mobile profile (capped at 720p, AV1/x264).
pub fn mobile_profile() -> Profile {
    Profile {
        name: "Mobile".into(),
        device: DeviceClass::Mobile,
        description: "Smartphones and small tablets. Screen <7 inches.".into(),
        vmaf_model: "vmaf_v0.6.1".into(),
        max_res: RES_720P,
        resolutions: vec![RES_360P, RES_480P, RES_720P],
        codecs: vec![Codec::SvtAv1, Codec::X264],
        ladder_opts: LadderOpts {
            num_rungs: 4,
            min_bitrate: 150.0,
            max_bitrate: 3000.0,
            min_vmaf: 50.0,
            max_vmaf: 95.0,
            audio_bitrate_kbps: 0.0,
        },
    }
}

/// Returns the standard Desktop profile (capped at 1080p, AV1/x265/x264).
pub fn desktop_profile() -> Profile {
    Profile {
        name: "Desktop".into(),
        device: DeviceClass::Desktop,
        description: "Laptops and desktop monitors. 13-27 inch screens.".into(),
        vmaf_model: "vmaf_v0.6.1".into(),
        max_res: RES_1080P,
        resolutions: vec![RES_480P, RES_720P, RES_1080P],
        codecs: vec![Codec::SvtAv1, Codec::X265, Codec::X264],
        ladder_opts: LadderOpts::default(),
    }
}

/// Returns the standard 1080p TV profile (8 rungs, AV1/x265/x264).
pub fn tv_profile() -> Profile {
    Profile {
        name: "TV (1080p)".into(),
        device: DeviceClass::Tv,
        description: "TVs and large displays. 40-65 inch screens.".into(),
        vmaf_model: "vmaf_v0.6.1".into(),
        max_res: RES_1080P,
        resolutions: vec![RES_480P, RES_720P, RES_1080P],
        codecs: vec![Codec::SvtAv1, Codec::X265, Codec::X264],
        ladder_opts: LadderOpts {
            num_rungs: 8,
            min_bitrate: 200.0,
            max_bitrate: 12000.0,
            min_vmaf: 40.0,
            max_vmaf: 0.0,
            audio_bitrate_kbps: 0.0,
        },
    }
}

/// Returns the standard 4K TV profile (up to 2160p, 4K VMAF model, AV1/x265).
pub fn tv_4k_profile() -> Profile {
    Profile {
        name: "TV (4K)".into(),
        device: DeviceClass::Tv4k,
        description: "4K TVs. 55-85 inch screens.".into(),
        vmaf_model: "vmaf_4k_v0.6.1".into(),
        max_res: RES_2160P,
        resolutions: vec![RES_720P, RES_1080P, RES_1440P, RES_2160P],
        codecs: vec![Codec::SvtAv1, Codec::X265],
        ladder_opts: LadderOpts {
            num_rungs: 8,
            min_bitrate: 1000.0,
            max_bitrate: 25000.0,
            min_vmaf: 40.0,
            max_vmaf: 97.0,
            audio_bitrate_kbps: 0.0,
        },
    }
}

/// Returns all standard profiles: Mobile, Desktop, TV (1080p), and TV (4K).
pub fn all_profiles() -> Vec<Profile> {
    vec![mobile_profile(), desktop_profile(), tv_profile(), tv_4k_profile()]
}
