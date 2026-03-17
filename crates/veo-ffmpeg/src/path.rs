use std::env;
use std::path::PathBuf;

/// Returns the path to the ffmpeg binary.
///
/// Resolution order:
/// 1. `VEO_FFMPEG` environment variable
/// 2. `bin/ffmpeg/ffmpeg` relative to the working directory
/// 3. `"ffmpeg"` (system PATH)
pub fn ffmpeg_path() -> String {
    if let Ok(p) = env::var("VEO_FFMPEG") {
        if !p.is_empty() {
            return p;
        }
    }
    if let Some(p) = local_binary("ffmpeg") {
        return p;
    }
    "ffmpeg".into()
}

/// Returns the path to the ffprobe binary.
///
/// Resolution order:
/// 1. `VEO_FFPROBE` environment variable
/// 2. `bin/ffmpeg/ffprobe` relative to the working directory
/// 3. `"ffprobe"` (system PATH)
pub fn ffprobe_path() -> String {
    if let Ok(p) = env::var("VEO_FFPROBE") {
        if !p.is_empty() {
            return p;
        }
    }
    if let Some(p) = local_binary("ffprobe") {
        return p;
    }
    "ffprobe".into()
}

fn local_binary(name: &str) -> Option<String> {
    let mut path = PathBuf::from("bin").join("ffmpeg");
    if cfg!(windows) {
        path = path.join(format!("{name}.exe"));
    } else {
        path = path.join(name);
    }
    if path.exists() {
        Some(path.to_string_lossy().into_owned())
    } else {
        None
    }
}
