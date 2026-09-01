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
            test_schedule,
            hide_window,
            quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Grindlewald");
}
