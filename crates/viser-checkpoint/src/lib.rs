use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use viser_hull::Point;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub config_hash: String,
    pub source: String,
    pub completed: HashMap<String, Point>,
}

/// Manages incremental trial result persistence.
pub struct Checkpoint {
    mu: Mutex<CheckpointInner>,
}

struct CheckpointInner {
    path: PathBuf,
    state: State,
    dirty: bool,
}

impl Checkpoint {
    /// Creates a checkpoint manager. If a checkpoint file exists with a matching
    /// config hash, completed trials are loaded.
    pub fn new(path: impl AsRef<Path>, config_hash: &str, source: &str) -> anyhow::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut state = State {
            config_hash: config_hash.to_string(),
            source: source.to_string(),
            completed: HashMap::new(),
        };

        if let Ok(data) = fs::read(&path) {
            if let Ok(existing) = serde_json::from_slice::<State>(&data) {
                if existing.config_hash == config_hash && existing.source == source {
                    state = existing;
                }
            }
        }

        Ok(Self { mu: Mutex::new(CheckpointInner { path, state, dirty: false }) })
    }

    pub fn is_completed(&self, resolution: &str, codec: &str, crf: i32) -> bool {
        let inner = self.mu.lock().unwrap();
        inner.state.completed.contains_key(&make_key(resolution, codec, crf))
    }

    pub fn get(&self, resolution: &str, codec: &str, crf: i32) -> Option<Point> {
        let inner = self.mu.lock().unwrap();
        inner.state.completed.get(&make_key(resolution, codec, crf)).cloned()
    }

    pub fn save(
        &self,
        resolution: &str,
        codec: &str,
        crf: i32,
        point: Point,
    ) -> anyhow::Result<()> {
        let mut inner = self.mu.lock().unwrap();
        inner.state.completed.insert(make_key(resolution, codec, crf), point);
        inner.dirty = true;
        flush_locked(&mut inner)
    }

    pub fn completed_count(&self) -> usize {
        let inner = self.mu.lock().unwrap();
        inner.state.completed.len()
    }

    pub fn all_completed(&self) -> Vec<Point> {
        let inner = self.mu.lock().unwrap();
        inner.state.completed.values().cloned().collect()
    }

    pub fn remove(&self) -> anyhow::Result<()> {
        let inner = self.mu.lock().unwrap();
        fs::remove_file(&inner.path)?;
        Ok(())
    }
}

fn flush_locked(inner: &mut CheckpointInner) -> anyhow::Result<()> {
    if !inner.dirty {
        return Ok(());
    }

    let data = serde_json::to_string_pretty(&inner.state)?;

    // Write atomically via temp file + rename
    let dir = inner.path.parent().unwrap_or(Path::new("."));
    let tmp = tempfile::NamedTempFile::new_in(dir)?;
    fs::write(tmp.path(), &data)?;
    tmp.persist(&inner.path)?;

    inner.dirty = false;
    Ok(())
}

/// Computes a deterministic hash of encoding configuration parameters.
pub fn config_hash(
    source: &str,
    resolutions: &[String],
    codecs: &[String],
    crf_values: &[i32],
    preset: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("source={source}\n"));
    for r in resolutions {
        hasher.update(format!("res={r}\n"));
    }
    for c in codecs {
        hasher.update(format!("codec={c}\n"));
    }
    for crf in crf_values {
        hasher.update(format!("crf={crf}\n"));
    }
    hasher.update(format!("preset={preset}\n"));
    let hash = hasher.finalize();
    hex::encode(&hash[..8])
}

/// Returns the default checkpoint file path for a given source.
pub fn default_path(source: &str) -> PathBuf {
    let path = Path::new(source);
    let dir = path.parent().unwrap_or(Path::new("."));
    let base = path.file_name().unwrap_or_default().to_string_lossy();
    dir.join(format!(".viser-checkpoint-{base}.json"))
}

fn make_key(resolution: &str, codec: &str, crf: i32) -> String {
    format!("{resolution}_{codec}_{crf}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use viser_ffmpeg::{Codec, Resolution};

    #[test]
    fn test_config_hash_deterministic() {
        let res = &["1920x1080".to_string(), "1280x720".to_string()];
        let codecs = &["libx264".to_string()];
        let crfs = &[22, 26, 30];
        let h1 = config_hash("video.mp4", res, codecs, crfs, "veryfast");
        let h2 = config_hash("video.mp4", res, codecs, crfs, "veryfast");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_config_hash_differs_on_source() {
        let res = &["1920x1080".to_string()];
        let codecs = &["libx264".to_string()];
        let crfs = &[23];
        let h1 = config_hash("a.mp4", res, codecs, crfs, "medium");
        let h2 = config_hash("b.mp4", res, codecs, crfs, "medium");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_config_hash_differs_on_preset() {
        let res = &["1920x1080".to_string()];
        let codecs = &["libx264".to_string()];
        let crfs = &[23];
        let h1 = config_hash("v.mp4", res, codecs, crfs, "veryfast");
        let h2 = config_hash("v.mp4", res, codecs, crfs, "slow");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_config_hash_differs_on_resolutions() {
        let codecs = &["libx264".to_string()];
        let crfs = &[23];
        let h1 = config_hash("v.mp4", &["1920x1080".to_string()], codecs, crfs, "veryfast");
        let h2 = config_hash("v.mp4", &["1280x720".to_string()], codecs, crfs, "veryfast");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_config_hash_hex() {
        let res = &["1920x1080".to_string()];
        let codecs = &["libx264".to_string()];
        let crfs = &[23];
        let h = config_hash("v.mp4", res, codecs, crfs, "veryfast");
        assert_eq!(h.len(), 16); // 8 bytes = 16 hex chars
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_default_path_keeps_basename() {
        let path = default_path("videos/input.mp4");
        assert!(path.to_string_lossy().contains("input.mp4"));
        assert!(path.to_string_lossy().starts_with("videos/"));
    }

    #[test]
    fn test_default_path_prefix() {
        let path = default_path("input.mp4");
        let name = path.to_string_lossy();
        assert!(name.contains(".viser-checkpoint-"));
    }

    #[test]
    fn test_make_key_format() {
        let key = make_key("1920x1080", "libx264", 23);
        assert_eq!(key, "1920x1080_libx264_23");
    }

    #[test]
    fn test_checkpoint_new_creates_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checkpoint.json");
        let cp = Checkpoint::new(&path, "abc123", "test.mp4").unwrap();
        assert_eq!(cp.completed_count(), 0);
    }

    #[test]
    fn test_checkpoint_loads_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checkpoint.json");
        let point = Point {
            resolution: Resolution::new(1920, 1080),
            codec: Codec::X264,
            crf: 23,
            bitrate: 1000.0,
            vmaf: 90.0,
            psnr: 0.0,
            ssim: 0.0,
        };
        // Create checkpoint, save a point
        let cp = Checkpoint::new(&path, "abc123", "test.mp4").unwrap();
        cp.save("1920x1080", "libx264", 23, point.clone()).unwrap();
        drop(cp);

        // Re-open with matching hash
        let cp2 = Checkpoint::new(&path, "abc123", "test.mp4").unwrap();
        assert_eq!(cp2.completed_count(), 1);
        assert!(cp2.is_completed("1920x1080", "libx264", 23));
        assert!(!cp2.is_completed("1920x1080", "libx264", 24));
        let loaded = cp2.get("1920x1080", "libx264", 23).unwrap();
        assert!((loaded.bitrate - 1000.0).abs() < 1e-9);
        cp2.remove().unwrap();
    }

    #[test]
    fn test_checkpoint_mismatched_hash_discards() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checkpoint.json");
        let cp = Checkpoint::new(&path, "abc123", "test.mp4").unwrap();
        cp.save("1920x1080", "libx264", 23, Point {
            resolution: Resolution::new(1920, 1080),
            codec: Codec::X264, crf: 23, bitrate: 1000.0, vmaf: 90.0, psnr: 0.0, ssim: 0.0,
        }).unwrap();
        drop(cp);

        // Re-open with different hash
        let cp2 = Checkpoint::new(&path, "def456", "test.mp4").unwrap();
        assert_eq!(cp2.completed_count(), 0);
        cp2.remove().unwrap();
    }

    #[test]
    fn test_checkpoint_all_completed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checkpoint.json");
        let cp = Checkpoint::new(&path, "hash", "src.mp4").unwrap();
        assert!(cp.all_completed().is_empty());

        let p = Point {
            resolution: Resolution::new(1920, 1080),
            codec: Codec::X264, crf: 23, bitrate: 1000.0, vmaf: 90.0, psnr: 0.0, ssim: 0.0,
        };
        cp.save("1920x1080", "libx264", 23, p).unwrap();
        assert_eq!(cp.all_completed().len(), 1);
        cp.remove().unwrap();
    }

    #[test]
    fn test_checkpoint_state_serialization() {
        let state = State {
            config_hash: "abc".into(),
            source: "test.mp4".into(),
            completed: std::collections::HashMap::new(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: State = serde_json::from_str(&json).unwrap();
        assert_eq!(back.config_hash, "abc");
        assert_eq!(back.source, "test.mp4");
    }
}
