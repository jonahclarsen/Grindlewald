pub mod ble;
pub mod breathing;
pub mod command;
pub mod ipc;
mod network;
pub mod privileged;
pub mod protocol;
pub mod settings;
pub mod state;

use std::{env, path::PathBuf};

use ble::DiscoveredDevice;
use command::ControlCommand;
use settings::Settings;
use state::SharedState;
use tauri::{
    Manager, PhysicalPosition,
    image::Image,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

fn tray_icon() -> Image<'static> {
    const SIZE: usize = 32;
    const SCALE: usize = 4;
    const LEFT: usize = 6;
    const TOP: usize = 2;
    const GLYPH: [&str; 7] = [
        "01110", "10001", "10000", "10111", "10001", "10001", "01110",
    ];

    let mut rgba = vec![0_u8; SIZE * SIZE * 4];
    for (row, line) in GLYPH.iter().enumerate() {
        for (column, pixel) in line.bytes().enumerate() {
            if pixel != b'1' {
                continue;
            }
            for y in 0..3 {
                for x in 0..3 {
                    let offset = ((TOP + row * SCALE + y) * SIZE + LEFT + column * SCALE + x) * 4;
                    rgba[offset + 3] = 255;
                }
            }
        }
    }
    Image::new_owned(rgba, SIZE as u32, SIZE as u32)
}

const TRAY_ID: &str = "main";

fn helper_alert_tooltip(status: &privileged::ServiceStatus) -> Option<&'static str> {
    if !status.installed {
        None
    } else if !status.healthy {
        Some("Grindlewald — Helper repair required")
    } else if !status.current {
        Some("Grindlewald — Helper update required")
    } else {
        None
    }
}

fn refresh_helper_tray(app: &tauri::AppHandle) -> privileged::ServiceStatus {
    let status = privileged::service_status();
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let tooltip = helper_alert_tooltip(&status);
        let icon = if tooltip.is_some() {
            Image::from_bytes(include_bytes!("../icons/helper-alert.png"))
                .expect("embedded helper alert icon must be a valid PNG")
        } else {
            tray_icon()
        };
        if let Err(error) = tray.set_icon(Some(icon)) {
            eprintln!("Could not update the menu bar icon: {error}");
        }
        // Preserve the alert color; let macOS adapt only the normal icon.
        if let Err(error) = tray.set_icon_as_template(tooltip.is_none()) {
            eprintln!("Could not set the menu bar icon appearance: {error}");
        }
        if let Err(error) = tray.set_tooltip(Some(tooltip.unwrap_or("Grindlewald"))) {
            eprintln!("Could not update the menu bar tooltip: {error}");
        }
    }
    status
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, SharedState>) -> Result<Settings, String> {
    state.load_settings()
}

#[tauri::command]
fn save_settings(settings: Settings, state: tauri::State<'_, SharedState>) -> Result<(), String> {
    state.save_settings(&settings)
}

#[tauri::command]
async fn discover_lights(
    state: tauri::State<'_, SharedState>,
) -> Result<Vec<DiscoveredDevice>, String> {
    state.discover().await
}

#[tauri::command]
async fn execute_control(
    command: ControlCommand,
    state: tauri::State<'_, SharedState>,
) -> Result<String, String> {
    state.execute(command).await
}

#[tauri::command]
async fn connection_status(
    state: tauri::State<'_, SharedState>,
) -> Result<state::ConnectionStatus, String> {
    state.connection_status().await
}

#[tauri::command]
async fn disconnect_lights(state: tauri::State<'_, SharedState>) -> Result<String, String> {
    state.disconnect().await
}

#[tauri::command]
async fn test_schedule(id: String, state: tauri::State<'_, SharedState>) -> Result<String, String> {
    state.run_schedule_by_id(&id).await
}

#[tauri::command]
fn privileged_service_status(app: tauri::AppHandle) -> privileged::ServiceStatus {
    refresh_helper_tray(&app)
}

#[tauri::command]
async fn install_privileged_service() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(privileged::install_service)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn approve_privileged_job(
    id: String,
    state: tauri::State<'_, SharedState>,
) -> Result<String, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.approve_privileged_job(&id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn revoke_privileged_job(
    id: String,
    state: tauri::State<'_, SharedState>,
) -> Result<String, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.revoke_privileged_job(&id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn uninstall_privileged_service(
    state: tauri::State<'_, SharedState>,
) -> Result<String, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let message = privileged::uninstall_service()?;
        state.clear_privileged_approvals()?;
        Ok(message)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn floodlight_script_path() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("GRINDLEWALD_FLOODLIGHT_SCRIPT").map(PathBuf::from)
        && path.is_file()
    {
        return Ok(path);
    }

    let mut candidates = Vec::new();
    if let Some(repo) = env::var_os("GRINDLEWALD_REPO").map(PathBuf::from) {
        candidates.push(
            repo.parent()
                .map(|path| path.join("shortcut_set_floodlights.py")),
        );
    }
    candidates.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|repo| repo.parent())
            .map(|path| path.join("shortcut_set_floodlights.py")),
    );

    candidates
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .ok_or_else(|| {
            "Floodlight script not found; set GRINDLEWALD_FLOODLIGHT_SCRIPT to its path".into()
        })
}

fn floodlight_python_path() -> PathBuf {
    if let Some(path) = env::var_os("GRINDLEWALD_FLOODLIGHT_PYTHON").map(PathBuf::from)
        && path.is_file()
    {
        return path;
    }
    if let Some(path) = dirs::home_dir()
        .map(|home| home.join("miniconda3/envs/govee/bin/python"))
        .filter(|path| path.is_file())
    {
        return path;
    }
    PathBuf::from("python3")
}

async fn run_floodlights(on: bool) -> Result<String, String> {
    let state = if on { "on" } else { "off" };
    eprintln!(
        "{} Grindlewald floodlight script starting: {state}",
        chrono::Local::now().to_rfc3339()
    );
    let output = tokio::process::Command::new(floodlight_python_path())
        .arg(floodlight_script_path()?)
        .arg(state)
        .output()
        .await
        .map_err(|error| format!("Could not start the floodlight script: {error}"))?;

    if output.status.success() {
        eprintln!(
            "{} Grindlewald floodlight script completed: {state}",
            chrono::Local::now().to_rfc3339()
        );
        Ok(format!("Floodlights {state}"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(if stderr.is_empty() {
            format!("Floodlight script exited with {}", output.status)
        } else {
            format!("Floodlight script failed: {stderr}")
        })
    }
}

#[tauri::command]
async fn set_floodlights(on: bool) -> Result<String, String> {
    run_floodlights(on).await
}

#[tauri::command]
async fn copy_error_details(text: String) -> Result<(), String> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;

    let mut child = tokio::process::Command::new("/usr/bin/pbcopy")
        .env("LC_CTYPE", "en_US.UTF-8")
        .stdin(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("Could not open the clipboard: {error}"))?;
    let mut stdin = child.stdin.take().ok_or("Clipboard input is unavailable")?;
    stdin
        .write_all(text.as_bytes())
        .await
        .map_err(|error| format!("Could not copy error details: {error}"))?;
    drop(stdin);
    let status = child.wait().await.map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("Could not copy error details. Select the text and press Command-C.".into())
    }
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn hide_window(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let state = SharedState::new(settings_path);
            state.start_scheduler();
            let socket_state = state.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = ipc::serve(socket_state).await {
                    eprintln!("Grindlewald CLI server stopped: {error}");
                }
            });
            app.manage(state);

            TrayIconBuilder::with_id(TRAY_ID)
                .icon(tray_icon())
                .icon_as_template(true)
                .tooltip("Grindlewald")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        position,
                        rect,
                        ..
                    } = event
                        && let Some(window) = tray.app_handle().get_webview_window("main")
                    {
                        let visible = window.is_visible().unwrap_or(false);
                        if visible {
                            let _ = window.hide();
                        } else {
                            let scale = window.scale_factor().unwrap_or(1.0);
                            let tray_position = rect.position.to_physical::<f64>(scale);
                            let tray_size = rect.size.to_physical::<f64>(scale);
                            let window_size = window.outer_size().unwrap_or_default();
                            let mut x = tray_position.x + tray_size.width / 2.0
                                - f64::from(window_size.width) / 2.0;
                            let mut y = tray_position.y + tray_size.height + 6.0 * scale;

                            if let Ok(Some(monitor)) =
                                window.monitor_from_point(position.x, position.y)
                            {
                                let monitor_position = monitor.position();
                                let monitor_size = monitor.size();
                                let minimum_x = f64::from(monitor_position.x);
                                let maximum_x = minimum_x + f64::from(monitor_size.width)
                                    - f64::from(window_size.width);
                                let minimum_y = f64::from(monitor_position.y);
                                let maximum_y = minimum_y + f64::from(monitor_size.height)
                                    - f64::from(window_size.height);
                                x = x.clamp(minimum_x, maximum_x.max(minimum_x));
                                y = y.clamp(minimum_y, maximum_y.max(minimum_y));
                            }
                            let _ = window.set_position(PhysicalPosition::new(
                                x.round() as i32,
                                y.round() as i32,
                            ));
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
            // Check even while the window is hidden or its frontend is still loading.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || refresh_helper_tray(&handle));
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            tauri::WindowEvent::Focused(false) => {
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            discover_lights,
            execute_control,
            connection_status,
            disconnect_lights,
            test_schedule,
            privileged_service_status,
            install_privileged_service,
            approve_privileged_job,
            revoke_privileged_job,
            uninstall_privileged_service,
            set_floodlights,
            network::home_network_status,
            copy_error_details,
            hide_window,
            quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Grindlewald");
}
