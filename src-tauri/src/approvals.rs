//! Approves several administrator commands with one macOS prompt.
//!
//! This lives outside `privileged.rs` on purpose: the installed helper's
//! version is a hash of that file, so editing it asks for a helper update.
//! It only calls the helper's existing `approve` operation once per job.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
};

use sha2::{Digest, Sha256};

use crate::privileged;

const HELPER_PATH: &str = "/Library/PrivilegedHelperTools/com.jonahclarsen.grindlewald.privileged";

#[derive(serde::Serialize)]
struct ApprovedJob<'a> {
    id: &'a str,
    command: &'a str,
}

pub fn approve_jobs(config_directory: &Path, jobs: &[(String, String)]) -> Result<(), String> {
    if jobs.is_empty() {
        return Ok(());
    }
    let status = privileged::service_status();
    if !status.installed || !status.healthy {
        return Err("install or repair the privileged automation service first".into());
    }
    fs::create_dir_all(config_directory).map_err(|error| error.to_string())?;
    let mut manifests: Vec<PathBuf> = Vec::new();
    let result = (|| {
        let mut steps = vec!["set -e".to_owned()];
        for (id, command) in jobs {
            validate(id, command)?;
            let manifest = serde_json::to_vec(&ApprovedJob { id, command })
                .map_err(|error| error.to_string())?;
            let path = config_directory.join(format!(
                ".privileged-approval-{}-{}.json",
                std::process::id(),
                fastrand::u64(..)
            ));
            manifests.push(path.clone());
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
                .map_err(|error| error.to_string())?;
            file.write_all(&manifest).map_err(|error| error.to_string())?;
            file.sync_all().map_err(|error| error.to_string())?;
            steps.push(approve_step(&path, &hex(&Sha256::digest(&manifest))));
        }
        authorize(&steps.join("; "))
    })();
    for path in manifests {
        let _ = fs::remove_file(path);
    }
    result
}

fn approve_step(manifest: &Path, digest: &str) -> String {
    [HELPER_PATH, "approve", manifest.to_string_lossy().as_ref(), digest]
        .iter()
        .map(|argument| shell_quote(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

fn authorize(script: &str) -> Result<(), String> {
    let output = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "do shell script (item 1 of argv) with administrator privileges",
            "-e",
            "end run",
            "--",
            script,
        ])
        .output()
        .map_err(|error| format!("could not open administrator authorization: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if message.contains("User canceled") || message.contains("-128") {
        Err("administrator authorization was canceled".into())
    } else {
        Err(format!("administrator authorization failed: {message}"))
    }
}

fn validate(id: &str, command: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 64
        || !id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("administrator job identifier is invalid".into());
    }
    if command.trim().is_empty() || command.len() > 64 * 1024 || command.contains('\0') {
        return Err("administrator command is empty, invalid or too large".into());
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_script_is_valid_sh_and_quotes_paths() {
        let script = [
            "set -e".to_owned(),
            approve_step(Path::new("/tmp/it's a manifest.json"), &"a".repeat(64)),
            approve_step(Path::new("/tmp/second.json"), &"b".repeat(64)),
        ]
        .join("; ");
        let status = Command::new("/bin/sh")
            .args(["-n", "-c", &script])
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(script.matches("' 'approve' '").count(), 2);
    }

    #[test]
    fn rejects_unsafe_ids_and_empty_commands() {
        assert!(validate("good-id-1", "true").is_ok());
        assert!(validate("../x", "true").is_err());
        assert!(validate("id", "  ").is_err());
    }
}
