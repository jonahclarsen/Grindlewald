use std::time::Duration;

fn parse_ssid(output: &str) -> Option<String> {
    let ssid = output
        .strip_prefix("Current Wi-Fi Network: ")?
        .trim_end_matches(['\r', '\n']);
    if ssid.is_empty() || ssid == "<redacted>" {
        None
    } else {
        Some(ssid.to_owned())
    }
}

pub(crate) fn matches_saved_ssid(saved: Option<&str>, current: Option<&str>) -> bool {
    matches!((saved, current), (Some(saved), Some(current)) if !saved.is_empty() && saved == current)
}

#[tauri::command]
pub async fn current_wifi_ssid() -> Result<Option<String>, String> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::process::Command::new("/usr/sbin/networksetup")
            .args(["-getairportnetwork", "en0"])
            .env("LC_ALL", "C")
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| "Wi-Fi lookup timed out".to_string())?
    .map_err(|error| format!("Could not check Wi-Fi: {error}"))?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(parse_ssid(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_available_network_names_without_changing_them() {
        assert_eq!(
            parse_ssid("Current Wi-Fi Network: Test: Wi-Fi \n"),
            Some("Test: Wi-Fi ".into())
        );
        assert_eq!(
            parse_ssid("You are not associated with an AirPort network.\n"),
            None
        );
        assert_eq!(parse_ssid("Current Wi-Fi Network: <redacted>\n"), None);
        assert_eq!(parse_ssid("Current Wi-Fi Network: \n"), None);
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
