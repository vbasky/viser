use serde::{Deserialize, Serialize};
use viser_ffmpeg::{
    Codec, RES_360P, RES_480P, RES_720P, RES_1080P, RES_1440P, RES_2160P, Resolution,
};
use viser_ladder::Opts as LadderOpts;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClass {
    Mobile,
    Desktop,
    Tv,
    Tv4k,
}

/// Encoding constraints and quality targets for a device class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    pub device: DeviceClass,
    pub description: String,
    pub vmaf_model: String,
    pub max_res: Resolution,
    pub resolutions: Vec<Resolution>,
    pub codecs: Vec<Codec>,
    pub ladder_opts: LadderOpts,
}

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
        },
    }
}

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
            max_vmaf: 97.0,
        },
    }
}

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
        },
    }
}

pub fn all_profiles() -> Vec<Profile> {
    vec![mobile_profile(), desktop_profile(), tv_profile(), tv_4k_profile()]
}
