use std::{
    ffi::CString,
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::ffi::OsStrExt,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const HELPER_PATH: &str = "/Library/PrivilegedHelperTools/com.jonahclarsen.grindlewald.privileged";
const JOBS_DIRECTORY: &str = "/Library/Application Support/Grindlewald/PrivilegedJobs";
const SUDOERS_PATH: &str = "/private/etc/sudoers.d/grindlewald";
const MAX_MANIFEST_BYTES: u64 = 128 * 1024;
const SAFE_PATH: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub installed: bool,
    pub healthy: bool,
    pub current: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ApprovedJob {
    id: String,
    command: String,
}

pub fn maybe_run_helper() -> Option<i32> {
    let arguments: Vec<String> = std::env::args().collect();
    let executable = std::env::current_exe().ok();
    if executable.as_deref() != Some(Path::new(HELPER_PATH)) {
        return None;
    }

    Some(match dispatch_helper(&arguments[1..]) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Grindlewald privileged helper: {error}");
            1
        }
    })
}

pub fn service_status() -> ServiceStatus {
    let metadata = match fs::symlink_metadata(HELPER_PATH) {
        Ok(metadata) => metadata,
        Err(_) => {
            return ServiceStatus {
                installed: false,
                healthy: false,
                current: false,
                message: "Not installed".into(),
            };
        }
    };
    let sudoers_metadata = fs::symlink_metadata(SUDOERS_PATH).ok();
    let jobs_metadata = fs::symlink_metadata(JOBS_DIRECTORY).ok();
    let jobs_parent_metadata = Path::new(JOBS_DIRECTORY)
        .parent()
        .and_then(|path| fs::symlink_metadata(path).ok());
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || !sudoers_metadata
            .as_ref()
            .is_some_and(|value| value.uid() == 0 && value.mode() & 0o022 == 0 && value.is_file())
        || !jobs_metadata
            .as_ref()
            .is_some_and(|value| value.uid() == 0 && value.mode() & 0o077 == 0 && value.is_dir())
        || !jobs_parent_metadata
            .as_ref()
            .is_some_and(|value| value.uid() == 0 && value.mode() & 0o022 == 0 && value.is_dir())
    {
        return ServiceStatus {
            installed: true,
            healthy: false,
            current: false,
            message: "Installation needs repair".into(),
        };
    }

    let current = std::env::current_exe()
        .ok()
        .and_then(|executable| sha256_file(&executable).ok())
        .zip(sha256_file(Path::new(HELPER_PATH)).ok())
        .is_some_and(|(application, helper)| application == helper);
    ServiceStatus {
        installed: true,
        healthy: true,
        current,
        message: if current {
            "Ready for unattended administrator jobs".into()
        } else {
            "Helper update available".into()
        },
    }
}

pub fn install_service() -> Result<String, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let digest = sha256_file(&executable)?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let username_output = Command::new("/usr/bin/id")
        .arg("-un")
        .output()
        .map_err(|error| format!("could not determine the current user: {error}"))?;
    if !username_output.status.success() {
        return Err("could not determine the current user".into());
    }
    let username = String::from_utf8_lossy(&username_output.stdout)
        .trim()
        .to_owned();
    let script = installation_script(&executable, &digest, &username)?;
    invoke_authorized_shell(&script)?;
    Ok("Privileged automation service installed".into())
}

fn installation_script(executable: &Path, digest: &str, username: &str) -> Result<String, String> {
    validate_username(username)?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("application digest is invalid".into());
    }
    let temporary_helper = format!("{HELPER_PATH}.new-{}", std::process::id());
    let source = shell_quote(executable.to_string_lossy().as_ref());
    let temporary = shell_quote(&temporary_helper);
    let destination = shell_quote(HELPER_PATH);
    let expected_digest = shell_quote(digest);
    let account = shell_quote(username);
    Ok(format!(
        "set -eu; temporary={temporary}; trap '/bin/rm -f -- \"$temporary\"' EXIT; \
         /usr/bin/install -o root -g wheel -m 0755 {source} \"$temporary\"; \
         actual=$(/usr/bin/shasum -a 256 \"$temporary\" | /usr/bin/cut -d ' ' -f 1); \
         test \"$actual\" = {expected_digest}; /bin/mv -f \"$temporary\" {destination}; \
         trap - EXIT; {destination} finish-install {account}"
    ))
}

pub fn approve_job(config_directory: &Path, id: &str, command: &str) -> Result<(), String> {
    validate_job(id, command)?;
    let status = service_status();
    if !status.installed || !status.healthy {
        return Err("install or repair the privileged automation service first".into());
    }
    fs::create_dir_all(config_directory).map_err(|error| error.to_string())?;
    let manifest = serde_json::to_vec(&ApprovedJob {
        id: id.into(),
        command: command.into(),
    })
    .map_err(|error| error.to_string())?;
    let digest = sha256_hex(&manifest);
    let manifest_path = config_directory.join(format!(
        ".privileged-approval-{}-{}.json",
        std::process::id(),
        fastrand::u64(..)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&manifest_path)
            .map_err(|error| error.to_string())?;
        file.write_all(&manifest)
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        invoke_authorized(&[
            HELPER_PATH,
            "approve",
            manifest_path.to_string_lossy().as_ref(),
            &digest,
        ])
    })();
    let _ = fs::remove_file(manifest_path);
    result
}

pub fn revoke_job(id: &str) -> Result<String, String> {
    validate_job_id(id)?;
    invoke_authorized(&[HELPER_PATH, "revoke", id])?;
    Ok("Administrator job approval revoked".into())
}

pub fn uninstall_service() -> Result<String, String> {
    if !Path::new(HELPER_PATH).is_file() {
        return Ok("Privileged automation service is not installed".into());
    }
    invoke_authorized(&[HELPER_PATH, "uninstall"])?;
    Ok("Privileged automation service removed".into())
}

pub async fn run_job(id: &str) -> Result<String, String> {
    validate_job_id(id)?;
    let output = tokio::process::Command::new("/usr/bin/sudo")
        .args(["-n", HELPER_PATH, "run", id])
        .output()
        .await
        .map_err(|error| format!("could not start privileged job: {error}"))?;
    if output.status.success() {
        Ok("Administrator command completed".into())
    } else {
        Err(output_error("administrator command failed", &output))
    }
}

fn dispatch_helper(arguments: &[String]) -> Result<i32, String> {
    match arguments {
        [operation] if operation == "status" => {
            print!("{}", helper_version());
            Ok(0)
        }
        [operation, username] if operation == "finish-install" => {
            require_root()?;
            finish_install_as_root(username)?;
            Ok(0)
        }
        [operation, manifest, digest] if operation == "approve" => {
            require_root()?;
            approve_as_root(Path::new(manifest), digest)?;
            Ok(0)
        }
        [operation, id] if operation == "revoke" => {
            require_root()?;
            revoke_as_root(id)?;
            Ok(0)
        }
        [operation, id] if operation == "run" => {
            require_root()?;
            run_as_root(id)
        }
        [operation] if operation == "uninstall" => {
            require_root()?;
            uninstall_as_root()?;
            Ok(0)
        }
        _ => Err("invalid operation or arguments".into()),
    }
}

fn require_root() -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        Ok(())
    } else {
        Err("this operation requires administrator authorization".into())
    }
}

fn finish_install_as_root(username: &str) -> Result<(), String> {
    validate_username(username)?;
    prepare_jobs_directory_as_root()?;

    let sudoers = sudoers_rule(username)?;
    let temporary_sudoers = format!("{SUDOERS_PATH}.new-{}", std::process::id());
    write_new_file(Path::new(&temporary_sudoers), sudoers.as_bytes(), 0o440)?;
    let validation = Command::new("/usr/sbin/visudo")
        .args(["-cf", &temporary_sudoers])
        .output()
        .map_err(|error| format!("could not validate sudo policy: {error}"))?;
    if !validation.status.success() {
        let _ = fs::remove_file(&temporary_sudoers);
        return Err(output_error("sudo policy validation failed", &validation));
    }
    fs::rename(&temporary_sudoers, SUDOERS_PATH).map_err(|error| error.to_string())?;
    Ok(())
}

fn approve_as_root(manifest_path: &Path, expected_digest: &str) -> Result<(), String> {
    if expected_digest.len() != 64 || !expected_digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("invalid approval digest".into());
    }
    let metadata = fs::metadata(manifest_path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_MANIFEST_BYTES || !metadata.is_file() {
        return Err("approval manifest is invalid or too large".into());
    }
    let mut manifest = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(manifest_path)
        .and_then(|mut file| file.read_to_end(&mut manifest))
        .map_err(|error| error.to_string())?;
    if sha256_hex(&manifest) != expected_digest.to_ascii_lowercase() {
        return Err("approval manifest changed before authorization completed".into());
    }
    let job: ApprovedJob = serde_json::from_slice(&manifest).map_err(|error| error.to_string())?;
    validate_job(&job.id, &job.command)?;
    prepare_jobs_directory_as_root()?;
    let contents = serde_json::to_vec_pretty(&job).map_err(|error| error.to_string())?;
    let destination = job_path(&job.id)?;
    let temporary = destination.with_extension(format!("json.new-{}", std::process::id()));
    write_new_file(&temporary, &contents, 0o600)?;
    fs::rename(temporary, destination).map_err(|error| error.to_string())
}

fn revoke_as_root(id: &str) -> Result<(), String> {
    let path = job_path(id)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn run_as_root(id: &str) -> Result<i32, String> {
    let path = job_path(id)?;
    let metadata = fs::metadata(&path).map_err(|_| "approved administrator job was not found")?;
    if metadata.uid() != 0 || metadata.mode() & 0o077 != 0 || !metadata.is_file() {
        return Err("approved administrator job has unsafe permissions".into());
    }
    let job: ApprovedJob = serde_json::from_slice(
        &fs::read(&path).map_err(|error| format!("could not read approved job: {error}"))?,
    )
    .map_err(|error| format!("approved job is invalid: {error}"))?;
    validate_job(&job.id, &job.command)?;
    if job.id != id {
        return Err("approved job identifier does not match its filename".into());
    }
    let status = Command::new("/bin/zsh")
        .args(["-lc", &job.command])
        .env_clear()
        .env("HOME", "/var/root")
        .env("PATH", SAFE_PATH)
        .env("SHELL", "/bin/zsh")
        .env("USER", "root")
        .current_dir("/")
        .status()
        .map_err(|error| format!("could not start approved command: {error}"))?;
    Ok(status.code().unwrap_or(1))
}

fn uninstall_as_root() -> Result<(), String> {
    if Path::new(SUDOERS_PATH).exists() {
        fs::remove_file(SUDOERS_PATH).map_err(|error| error.to_string())?;
    }
    if Path::new(JOBS_DIRECTORY).exists() {
        fs::remove_dir_all(JOBS_DIRECTORY).map_err(|error| error.to_string())?;
    }
    if let Some(parent) = Path::new(JOBS_DIRECTORY).parent() {
        let _ = fs::remove_dir(parent);
    }
    fs::remove_file(HELPER_PATH).map_err(|error| error.to_string())
}

fn invoke_authorized(arguments: &[&str]) -> Result<(), String> {
    let command = arguments
        .iter()
        .map(|argument| shell_quote(argument))
        .collect::<Vec<_>>()
        .join(" ");
    invoke_authorized_shell(&command)
}

fn invoke_authorized_shell(command: &str) -> Result<(), String> {
    let output = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "do shell script (item 1 of argv) with administrator privileges",
            "-e",
            "end run",
            "--",
            command,
        ])
        .output()
        .map_err(|error| format!("could not open administrator authorization: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let message = String::from_utf8_lossy(&output.stderr);
        if message.contains("User canceled") || message.contains("-128") {
            Err("administrator authorization was canceled".into())
        } else {
            Err(output_error("administrator authorization failed", &output))
        }
    }
}

fn validate_username(username: &str) -> Result<(), String> {
    if username.is_empty()
        || username.len() > 64
        || !username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        Err("current account name cannot be represented safely in sudo policy".into())
    } else {
        Ok(())
    }
}

fn sudoers_rule(username: &str) -> Result<String, String> {
    validate_username(username)?;
    Ok(format!(
        "{username} ALL=(root) NOPASSWD: {HELPER_PATH} ^run [A-Za-z0-9-]{{1,64}}$\n"
    ))
}

fn validate_job(id: &str, command: &str) -> Result<(), String> {
    validate_job_id(id)?;
    if command.trim().is_empty() {
        return Err("administrator command cannot be empty".into());
    }
    if command.len() > 64 * 1024 || command.as_bytes().contains(&0) {
        return Err("administrator command is invalid or too large".into());
    }
    Ok(())
}

fn validate_job_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Err("administrator job identifier is invalid".into())
    } else {
        Ok(())
    }
}

fn job_path(id: &str) -> Result<PathBuf, String> {
    validate_job_id(id)?;
    Ok(Path::new(JOBS_DIRECTORY).join(format!("{id}.json")))
}

fn write_new_file(path: &Path, contents: &[u8], mode: u32) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|error| error.to_string())?;
    file.write_all(contents)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
}

fn prepare_jobs_directory_as_root() -> Result<(), String> {
    let parent = Path::new(JOBS_DIRECTORY)
        .parent()
        .ok_or("privileged jobs directory has no parent")?;
    prepare_root_directory(parent, 0o755)?;
    prepare_root_directory(Path::new(JOBS_DIRECTORY), 0o700)
}

fn prepare_root_directory(path: &Path, mode: u32) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_dir() => {
            return Err(format!("{} is not a safe directory", path.display()));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|error| error.to_string())?;
        }
        Err(error) => return Err(error.to_string()),
    }
    set_root_ownership(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|error| error.to_string())
}

fn set_root_ownership(path: &Path) -> Result<(), String> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "privileged path contains a null byte")?;
    if unsafe { libc::chown(path.as_ptr(), 0, 0) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

fn sha256_hex(contents: &[u8]) -> String {
    Sha256::digest(contents)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sha256_file(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes_read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }
    Ok(hasher.finalize().to_vec())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn helper_version() -> String {
    format!("{}\n", env!("CARGO_PKG_VERSION"))
}

fn output_error(context: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        format!("{context}: {}", output.status)
    } else {
        format!("{context}: {stderr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_ids_and_usernames_reject_shell_syntax_and_paths() {
        assert!(validate_job_id("550e8400-e29b-41d4-a716-446655440000").is_ok());
        assert!(validate_job_id("../../sudoers").is_err());
        assert!(validate_job_id("job;touch-x").is_err());
        assert!(validate_username("sample.user").is_ok());
        assert!(validate_username("sample ALL=(ALL)").is_err());
    }

    #[test]
    fn shell_arguments_are_single_quoted_safely() {
        assert_eq!(shell_quote("plain"), "'plain'");
        assert_eq!(shell_quote("it's safe"), "'it'\\''s safe'");
    }

    #[test]
    fn manifest_digest_changes_with_the_approved_command() {
        assert_ne!(sha256_hex(b"first"), sha256_hex(b"second"));
        assert_eq!(sha256_hex(b"first").len(), 64);
    }

    #[test]
    fn sudo_policy_only_allows_one_validated_run_identifier() {
        let rule = sudoers_rule("sampleuser").unwrap();
        assert!(rule.contains("^run [A-Za-z0-9-]{1,64}$"));
        assert!(!rule.contains("approve"));
        assert!(!rule.contains("uninstall"));
    }

    #[test]
    fn generated_installation_script_is_valid_zsh() {
        let script = installation_script(
            Path::new("/Applications/Grindlewald's Test.app/Contents/MacOS/grindlewald"),
            &"a".repeat(64),
            "sampleuser",
        )
        .unwrap();
        let status = Command::new("/bin/zsh")
            .args(["-n", "-c", &script])
            .status()
            .unwrap();
        assert!(status.success());
        assert!(script.contains("finish-install"));
    }
}
