//! Android exposes lighting and settings only. No scheduler, shell, or helper commands.
use crate::{ble::DiscoveredDevice, command::ControlCommand, settings::Settings, state::SharedState};
use jni::{EnvUnowned, JavaVM, errors::{Error, ThrowRuntimeExAndDefault}, jni_sig, jni_str,
    objects::{Global, JClass, JString, JValue}, strings::JNIString};
use std::sync::OnceLock;
use tauri::{Emitter, Manager};
static BRIDGE: OnceLock<Global<JClass<'static>>> = OnceLock::new();

#[unsafe(no_mangle)]
pub extern "system" fn Java_com_jonahclarsen_grindlewald_MobileBridge_initialize<'local>(mut env: EnvUnowned<'local>, class: JClass<'local>) {
    env.with_env(|env| -> jni::errors::Result<()> {
        if let Err(error) = btleplug::platform::init(env) {
            return env.throw_new(jni_str!("java/lang/IllegalStateException"), JNIString::from(error.to_string()));
        }
        let _ = BRIDGE.set(env.new_global_ref(class)?);
        Ok(())
    }).resolve::<ThrowRuntimeExAndDefault>()
}
fn mobile_call(method: &str, argument: &str) -> Result<String, String> {
    let class = BRIDGE.get().ok_or("Android is still starting")?;
    let vm = JavaVM::singleton().map_err(|e| e.to_string())?;
    vm.attach_current_thread(|env| -> jni::errors::Result<String> {
        let argument = env.new_string(argument)?;
        let value = env.call_static_method(class, JNIString::from(method),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"), &[JValue::Object(&argument)])?.l()?;
        env.cast_local::<JString>(value)?.try_to_string(env)
    }).map_err(|e| match e {
        Error::JavaException | Error::CaughtJavaException { .. } => "Android could not complete the request".into(),
        e => e.to_string(),
    })
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
