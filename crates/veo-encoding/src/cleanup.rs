use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};
use tracing::debug;

/// Removes orphaned VEO temp directories older than `max_age`.
/// Called at startup to clean up after crashes or SIGKILL.
pub fn clean_stale_temp_dirs(max_age: Duration) {
    let tmp = std::env::temp_dir();
    clean_temp_dirs_in_root(&tmp, max_age);
}

fn clean_temp_dirs_in_root(root: &Path, max_age: Duration) {
    let entries = match fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return,
    };

    let now = SystemTime::now();

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else { continue };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("veo-") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else { continue };
        let Ok(modified) = metadata.modified() else { continue };
        if let Ok(age) = now.duration_since(modified) {
            if age > max_age {
                let path = entry.path();
                debug!("cleaning stale temp dir: {}", path.display());
                let _ = fs::remove_dir_all(&path);
            }
        }
    }
}
