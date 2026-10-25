//! Android exposes lighting and settings only. No scheduler, shell, or helper commands.
use crate::{ble::DiscoveredDevice, command::ControlCommand, settings::Settings, state::SharedState};
use jni::{JNIEnv, JavaVM, objects::{GlobalRef, JClass, JString, JValue}};
use std::sync::OnceLock;
use tauri::{Emitter, Manager};
static VM: OnceLock<JavaVM> = OnceLock::new();
static BRIDGE: OnceLock<GlobalRef> = OnceLock::new();

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_jonahclarsen_grindlewald_MobileBridge_initialize(env: JNIEnv, class: JClass) {
    if let Err(error) = btleplug::platform::init(&env) {
        let _ = env.throw_new("java/lang/IllegalStateException", error.to_string());
        return;
    }
    if let Ok(vm) = env.get_java_vm() { let _ = VM.set(vm); }
    if let Ok(class) = env.new_global_ref(class) { let _ = BRIDGE.set(class); }
}
fn mobile_call(method: &str, argument: &str) -> Result<String, String> {
    let vm = VM.get().ok_or("Android is still starting")?;
    let env = vm.attach_current_thread().map_err(|e| e.to_string())?;
    let class = BRIDGE.get().ok_or("Android is still starting")?;
    let argument = env.new_string(argument).map_err(|e| e.to_string())?;
    let result = env.call_static_method(JClass::from(class.as_obj()), method,
        "(Ljava/lang/String;)Ljava/lang/String;", &[JValue::Object(argument.into())]);
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
        return Err("Android could not complete the request".into());
    }
    let value = result.and_then(|v| v.l()).map_err(|e| e.to_string())?;
    env.get_string(JString::from(value)).map(String::from).map_err(|e| e.to_string())
}
#[tauri::command]
async fn mobile_action(action: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || mobile_call("action", &action)).await.map_err(|e|e.to_string())?
}
#[tauri::command]
fn get_settings(state: tauri::State<'_, SharedState>) -> Result<Settings, String> {
    let mut settings = state.load_settings()?; settings.schedules.clear(); Ok(settings)
}
#[tauri::command]
fn save_settings(mut settings: Settings, state: tauri::State<'_, SharedState>) -> Result<(), String> {
    settings.schedules.clear(); state.save_settings(&settings)
}
fn bluetooth_permission() -> Result<(), String> {
    let message = mobile_call("action", "bluetooth")?;
    if message.is_empty() { Ok(()) } else { Err(message) }
}
#[tauri::command]
async fn discover_lights(state: tauri::State<'_, SharedState>) -> Result<Vec<DiscoveredDevice>, String> {
    bluetooth_permission()?; state.discover().await
}
#[tauri::command]
async fn execute_control(command: ControlCommand, state: tauri::State<'_, SharedState>) -> Result<String, String> {
    bluetooth_permission()?; state.execute(command).await
}
#[tauri::command]
async fn connection_status(state: tauri::State<'_, SharedState>) -> Result<crate::state::ConnectionStatus, String> {
    state.connection_status().await
}
#[tauri::command]
async fn disconnect_lights(state: tauri::State<'_, SharedState>) -> Result<String, String> { state.disconnect().await }
#[tauri::command]
fn copy_error_details(text: String) -> Result<(), String> { mobile_call("copy", &text).map(|_| ()) }
#[tauri::mobile_entry_point]
pub fn run() {
    tauri::Builder::default().setup(|app| {
        let state = SharedState::new(app.path().app_config_dir()?.join("settings.json"));
        let mut frames = state.subscribe_breathing();
        let handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            while frames.changed().await.is_ok() {
                let _ = handle.emit("breathing-frame", frames.borrow_and_update().clone());
            }
        });
        let mut selections = state.subscribe_light_selection();
        let handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            while selections.changed().await.is_ok() {
                let _ = handle.emit("light-selection", selections.borrow_and_update().clone());
            }
        });
        app.manage(state); Ok(())
    }).invoke_handler(tauri::generate_handler![get_settings, save_settings, discover_lights,
        execute_control, connection_status, disconnect_lights, mobile_action, copy_error_details,
        crate::sync_store::load_sync_state, crate::sync_store::save_sync_state])
      .run(tauri::generate_context!()).expect("error running Grindlewald");
}
