use std::{fs, path::PathBuf};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

use crate::{
    command::{CommandResponse, ControlCommand},
    state::SharedState,
};

pub fn socket_path() -> PathBuf {
    std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("grindlewald.sock")
}

pub async fn serve(state: SharedState) -> Result<(), String> {
    let path = socket_path();
    if path.exists() {
        fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    let listener = UnixListener::bind(&path).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }

    loop {
        let (stream, _) = listener.accept().await.map_err(|error| error.to_string())?;
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(error) = handle(stream, state).await {
                eprintln!("Grindlewald CLI request failed: {error}");
            }
        });
    }
}

async fn handle(stream: UnixStream, state: SharedState) -> Result<(), String> {
    let (reader, mut writer) = stream.into_split();
    let mut request = String::new();
    BufReader::new(reader)
        .read_line(&mut request)
        .await
        .map_err(|error| error.to_string())?;
    let command: ControlCommand =
        serde_json::from_str(&request).map_err(|error| error.to_string())?;
    let response = match state.execute(command).await {
        Ok(message) => CommandResponse { ok: true, message },
        Err(message) => CommandResponse { ok: false, message },
    };
    let mut json = serde_json::to_vec(&response).map_err(|error| error.to_string())?;
    json.push(b'\n');
    writer
        .write_all(&json)
        .await
        .map_err(|error| error.to_string())
}
