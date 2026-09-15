use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WifiState {
    Connected,
    NotConnected,
    PermissionNeeded,
    PermissionDenied,
    LocationDisabled,
    Restricted,
}

#[derive(Debug, Serialize)]
pub struct WifiStatus {
    pub state: WifiState,
    pub ssid: Option<String>,
}

impl WifiStatus {
    fn into_ssid(self) -> Result<Option<String>, String> {
        match self.state {
            WifiState::Connected | WifiState::NotConnected => Ok(self.ssid),
            WifiState::PermissionNeeded => Err("Open Automations and allow Grindlewald to use Location Services to read Wi-Fi names.".into()),
            WifiState::PermissionDenied => Err("Allow Grindlewald in System Settings > Privacy & Security > Location Services to read Wi-Fi names.".into()),
            WifiState::LocationDisabled => Err("Enable Location Services in System Settings > Privacy & Security to read Wi-Fi names.".into()),
            WifiState::Restricted => Err("Location access is restricted on this Mac; Grindlewald cannot read Wi-Fi names.".into()),
        }
    }
}

#[cfg(target_os = "macos")]
fn native_status(request_permission: bool) -> Result<WifiStatus, String> {
    use std::ffi::{CStr, c_char};
    unsafe extern "C" {
        fn grindlewald_wifi_snapshot(request_permission: bool, ssid: *mut *mut c_char) -> i32;
    }
    let mut ssid = std::ptr::null_mut();
    // The bridge returns a strdup allocation, which we copy and free exactly once.
    let code = unsafe { grindlewald_wifi_snapshot(request_permission, &mut ssid) };
    let ssid = if ssid.is_null() {
        None
    } else {
        let value = unsafe { CStr::from_ptr(ssid) }
            .to_string_lossy()
            .into_owned();
        unsafe { libc::free(ssid.cast()) };
        Some(value)
    };
    let state = match code {
        0 => WifiState::Connected,
        1 => WifiState::NotConnected,
        2 => WifiState::PermissionNeeded,
        3 => WifiState::PermissionDenied,
        4 => WifiState::LocationDisabled,
        5 => WifiState::Restricted,
        _ => return Err("Could not read Wi-Fi information".into()),
    };
    Ok(WifiStatus { state, ssid })
}

#[cfg(not(target_os = "macos"))]
fn native_status(_: bool) -> Result<WifiStatus, String> {
    Err("Wi-Fi detection is only supported on macOS".into())
}

#[tauri::command]
pub async fn wifi_status(request_permission: Option<bool>) -> Result<WifiStatus, String> {
    tokio::task::spawn_blocking(move || native_status(request_permission.unwrap_or(false)))
        .await
        .map_err(|error| format!("Wi-Fi lookup failed: {error}"))?
}

// Scheduled execution checks permission but never opens a permission prompt.
pub async fn current_wifi_ssid() -> Result<Option<String>, String> {
    wifi_status(Some(false)).await?.into_ssid()
}

pub(crate) fn matches_saved_ssid(saved: Option<&str>, current: Option<&str>) -> bool {
    matches!((saved, current), (Some(saved), Some(current)) if !saved.is_empty() && saved == current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_failures_are_distinct_from_disconnected_wifi() {
        for state in [
            WifiState::PermissionNeeded,
            WifiState::PermissionDenied,
            WifiState::LocationDisabled,
            WifiState::Restricted,
        ] {
            assert!(WifiStatus { state, ssid: None }.into_ssid().is_err());
        }
        assert_eq!(
            WifiStatus {
                state: WifiState::NotConnected,
                ssid: None
            }
            .into_ssid()
            .unwrap(),
            None
        );
        assert_eq!(
            WifiStatus {
                state: WifiState::Connected,
                ssid: Some("Test Network".into())
            }
            .into_ssid()
            .unwrap()
            .as_deref(),
            Some("Test Network")
        );
    }

    #[test]
    fn requires_an_exact_nonempty_network_match() {
        assert!(matches_saved_ssid(Some("Test Wi-Fi"), Some("Test Wi-Fi")));
        assert!(!matches_saved_ssid(
            Some("Test Wi-Fi"),
            Some("Test Wi-Fi guest")
        ));
        assert!(!matches_saved_ssid(Some("Test Wi-Fi"), None));
        assert!(!matches_saved_ssid(None, None));
        assert!(!matches_saved_ssid(Some(""), Some("")));
    }
}
