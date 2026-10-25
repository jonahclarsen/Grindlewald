//! Pairing credentials and reconciliation metadata must survive abrupt WebView/process shutdown.
use std::{fs, io::Write, path::Path, sync::Mutex};
use crate::state::SharedState;
static WRITE_LOCK: Mutex<()> = Mutex::new(());
fn load(path: &Path) -> Result<serde_json::Value, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::Value::Null),
        Err(error) => Err(error.to_string()),
    }
}
fn save(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    if bytes.len() > 512_000 { return Err("Sync metadata is too large".into()); }
    let _guard = WRITE_LOCK.lock().map_err(|e| e.to_string())?;
    let parent = path.parent().ok_or("Missing configuration directory")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
    fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    fs::File::open(parent).and_then(|directory| directory.sync_all()).map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub fn load_sync_state(state: tauri::State<'_, SharedState>) -> Result<serde_json::Value, String> {
    load(&state.settings_path().with_file_name("sync.json"))
}
#[tauri::command]
pub fn save_sync_state(value: serde_json::Value, state: tauri::State<'_, SharedState>) -> Result<(), String> {
    save(&state.settings_path().with_file_name("sync.json"), &value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durable_state_round_trips_and_failed_write_preserves_previous_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("sync.json");
        assert_eq!(load(&path).unwrap(), serde_json::Value::Null);
        let value = serde_json::json!({"token":"synthetic-pairing", "model":{"clock":7}});
        save(&path, &value).unwrap();
        assert_eq!(load(&path).unwrap(), value);
        assert!(save(&path, &serde_json::json!({"oversized":"x".repeat(512_001)})).is_err());
        assert_eq!(load(&path).unwrap(), value);
    }
}
