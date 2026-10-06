use std::path::Path;

use crate::settings::SystemAppearance;

fn appearance_script(appearance: SystemAppearance) -> Option<&'static str> {
    match appearance {
        SystemAppearance::Unchanged => None,
        SystemAppearance::Light => Some(
            "tell application \"System Events\" to tell appearance preferences to set dark mode to false",
        ),
        SystemAppearance::Dark => Some(
            "tell application \"System Events\" to tell appearance preferences to set dark mode to true",
        ),
    }
}

pub async fn apply(appearance: SystemAppearance) -> Result<Option<String>, String> {
    apply_using(appearance, Path::new("/usr/bin/osascript")).await
}

async fn apply_using(
    appearance: SystemAppearance,
    program: &Path,
) -> Result<Option<String>, String> {
    let Some(script) = appearance_script(appearance) else {
        return Ok(None);
    };
    let output = tokio::process::Command::new(program)
        .args(["-e", script])
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| format!("could not set system appearance: {error}"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        let error = error.trim();
        if error.contains("-1743") {
            return Err("System appearance permission denied. Allow Grindlewald to control System Events in System Settings > Privacy & Security > Automation, then test the automation again.".into());
        }
        return Err(format!(
            "could not set system appearance: {}",
            if error.is_empty() {
                output.status.to_string()
            } else {
                error.to_owned()
            }
        ));
    }
    let mode = match appearance {
        SystemAppearance::Light => "Light",
        SystemAppearance::Dark => "Dark",
        SystemAppearance::Unchanged => unreachable!(),
    };
    Ok(Some(format!("System appearance set to {mode}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn fake_osascript(directory: &Path, body: &str) -> std::path::PathBuf {
        let path = directory.join("osascript");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[tokio::test]
    async fn unchanged_never_starts_a_process() {
        assert_eq!(
            apply_using(SystemAppearance::Unchanged, Path::new("/missing-osascript"))
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn light_and_dark_use_explicit_values_and_report_success() {
        let directory = tempfile::tempdir().unwrap();
        let program = fake_osascript(directory.path(), "printf '%s\\n' \"$@\" > \"$0.args\"");
        for (appearance, value, label) in [
            (SystemAppearance::Light, "false", "Light"),
            (SystemAppearance::Dark, "true", "Dark"),
        ] {
            let result = apply_using(appearance, &program).await.unwrap();
            assert_eq!(result, Some(format!("System appearance set to {label}")));
            let args = std::fs::read_to_string(directory.path().join("osascript.args")).unwrap();
            assert_eq!(
                args,
                format!(
                    "-e\ntell application \"System Events\" to tell appearance preferences to set dark mode to {value}\n"
                )
            );
        }
    }

    #[tokio::test]
    async fn permission_denial_explains_how_to_enable_automation() {
        let directory = tempfile::tempdir().unwrap();
        let program = fake_osascript(
            directory.path(),
            "echo 'Not authorized to send Apple events to System Events. (-1743)' >&2; exit 1",
        );
        let error = apply_using(SystemAppearance::Dark, &program)
            .await
            .unwrap_err();
        assert!(error.contains("Privacy & Security > Automation"));
        assert!(error.contains("System Events"));
    }

    #[tokio::test]
    async fn process_failures_are_reported() {
        let directory = tempfile::tempdir().unwrap();
        let program = fake_osascript(directory.path(), "echo 'script failed' >&2; exit 1");
        let error = apply_using(SystemAppearance::Light, &program)
            .await
            .unwrap_err();
        assert!(error.contains("script failed"));
        assert!(
            apply_using(SystemAppearance::Dark, Path::new("/missing-osascript"))
                .await
                .is_err()
        );
    }
}
