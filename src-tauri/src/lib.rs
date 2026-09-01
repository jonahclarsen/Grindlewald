pub mod ble;
pub mod command;
pub mod ipc;
pub mod protocol;
pub mod settings;
pub mod state;

use ble::DiscoveredDevice;
use command::ControlCommand;
use settings::Settings;
use state::SharedState;
use tauri::{
    Manager,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

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
async fn test_schedule(id: String, state: tauri::State<'_, SharedState>) -> Result<String, String> {
    state.run_schedule_by_id(&id).await
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

            TrayIconBuilder::new()
                .icon(app.default_window_icon().expect("app icon").clone())
                .icon_as_template(true)
                .tooltip("Grindlewald")
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                        && let Some(window) = tray.app_handle().get_webview_window("main")
                    {
                        let visible = window.is_visible().unwrap_or(false);
                        if visible {
                            let _ = window.hide();
                        } else {
                            let _ = window.center();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            discover_lights,
            execute_control,
            test_schedule,
            hide_window,
            quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Grindlewald");
}
