#[cfg(not(target_os = "android"))]
mod activity;
#[cfg(not(target_os = "android"))]
mod appearance;
pub mod ble;
pub mod breathing;
pub mod command;
#[cfg(not(target_os = "android"))]
pub mod ipc;
#[cfg(not(target_os = "android"))]
mod network;
#[cfg(not(target_os = "android"))]
pub mod privileged;
pub mod protocol;
#[cfg(not(target_os = "android"))]
mod scheduler;
pub mod settings;
pub mod spacing;
pub mod state;
mod timing;
mod sync_store;

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(not(target_os = "android"))]
pub use desktop::run;
#[cfg(not(target_os = "android"))]
pub(crate) use desktop::run_floodlights;
#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::run;
