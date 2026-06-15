use std::env;
use std::path::PathBuf;
use std::process::Command;

/// Returns the path to the ffmpeg binary.
///
/// Resolution order:
/// 1. `VISER_FFMPEG` environment variable
/// 2. `bin/ffmpeg/ffmpeg` relative to the working directory
/// 3. `"ffmpeg"` (system PATH)
pub fn ffmpeg_path() -> String {
    if let Ok(p) = env::var("VISER_FFMPEG")
        && !p.is_empty()
    {
        return p;
    }
    if let Some(p) = local_binary("ffmpeg") {
        return p;
    }
    "ffmpeg".into()
}

/// Returns the path to the ffprobe binary.
///
/// Resolution order:
/// 1. `VISER_FFPROBE` environment variable
/// 2. `bin/ffmpeg/ffprobe` relative to the working directory
/// 3. `"ffprobe"` (system PATH)
pub fn ffprobe_path() -> String {
    if let Ok(p) = env::var("VISER_FFPROBE")
        && !p.is_empty()
    {
        return p;
    }
    if let Some(p) = local_binary("ffprobe") {
        return p;
    }
    "ffprobe".into()
}

/// Minimum FFmpeg version required (major).
const MIN_FFMPEG_MAJOR: u32 = 6;

/// Parsed FFmpeg version.
#[derive(Debug, Clone)]
pub struct FfmpegVersion {
    /// Path to the binary that reported this version.
    pub binary: String,
    /// Major version number.
    pub major: u32,
    /// Minor version number.
    pub minor: u32,
    /// Raw version string as parsed from the binary's output.
    pub raw: String,
}

/// Run `ffmpeg -version` and parse the version line. Returns an error if the
/// binary is not found or the version is too old.
pub fn check_ffmpeg() -> anyhow::Result<FfmpegVersion> {
    let path = ffmpeg_path();
    let output = Command::new(&path)
        .arg("-version")
        .output()
        .map_err(|e| anyhow::anyhow!("ffmpeg not found at '{path}': {e}"))?;
    if !output.status.success() {
        anyhow::bail!("ffmpeg at '{path}' exited with error");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("").trim().to_string();
    let version = parse_ffmpeg_version(&first_line, path)?;
    if version.major < MIN_FFMPEG_MAJOR {
        anyhow::bail!(
            "ffmpeg {}.{} is too old — viser requires FFmpeg >= {MIN_FFMPEG_MAJOR}.0 (found {})",
            version.major,
            version.minor,
            version.raw,
        );
    }
    Ok(version)
}

/// Run `ffprobe -version` and parse the version line. Returns an error if ffprobe
/// is not found.
pub fn check_ffprobe() -> anyhow::Result<FfmpegVersion> {
    let path = ffprobe_path();
    let output = Command::new(&path)
        .arg("-version")
        .output()
        .map_err(|e| anyhow::anyhow!("ffprobe not found at '{path}': {e}"))?;
    if !output.status.success() {
        anyhow::bail!("ffprobe at '{path}' exited with error");
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("").trim().to_string();
    parse_ffmpeg_version(&first_line, path)
}

fn parse_ffmpeg_version(line: &str, path: String) -> anyhow::Result<FfmpegVersion> {
    // Typical first line: "ffmpeg version 7.1.1 Copyright ..."
    // or "ffmpeg version n7.1.1-... Copyright ..."
    let version_str = line
        .strip_prefix("ffmpeg version ")
        .or_else(|| line.strip_prefix("ffprobe version "))
        .and_then(|s| s.split_whitespace().next())
        .map(|s| s.trim_start_matches('n'))
        .ok_or_else(|| anyhow::anyhow!("could not parse version from: {line}"))?;
    let parts: Vec<&str> = version_str.split('.').collect();
    let major: u32 = parts
        .first()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| anyhow::anyhow!("could not parse major version from: {version_str}"))?;
    let minor: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok(FfmpegVersion { binary: path, major, minor, raw: version_str.to_string() })
}

/// Known valid libvmaf model names. Models are resolved by FFmpeg's built-in
/// libvmaf library; these are the names FFmpeg recognizes.
const KNOWN_VMAF_MODELS: &[&str] =
    &["vmaf_v0.6.1", "vmaf_v0.6.1neg", "vmaf_4k_v0.6.1", "vmaf_b_v0.6.3", "vmaf_4k_v0.6.1neg"];

/// Validate that the given VMAF model name is recognized by libvmaf.
pub fn validate_vmaf_model(model: &str) -> anyhow::Result<()> {
    if KNOWN_VMAF_MODELS.contains(&model) {
        return Ok(());
    }
    anyhow::bail!("unknown VMAF model '{model}'. Known models: {}", KNOWN_VMAF_MODELS.join(", "));
}

fn local_binary(name: &str) -> Option<String> {
    let mut path = PathBuf::from("bin").join("ffmpeg");
    if cfg!(windows) {
        path = path.join(format!("{name}.exe"));
    } else {
        path = path.join(name);
    }
    if path.exists() { Some(path.to_string_lossy().into_owned()) } else { None }
}
