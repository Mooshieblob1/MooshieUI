pub mod api;
#[cfg(feature = "desktop")]
pub mod config;
#[cfg(feature = "desktop")]
pub mod interrogator;
#[cfg(any(feature = "desktop", feature = "server"))]
pub mod prompt_assistant;
#[cfg(feature = "desktop")]
pub mod server;
#[cfg(feature = "desktop")]
pub mod websocket;
#[cfg(feature = "desktop")]
pub mod workflow;
