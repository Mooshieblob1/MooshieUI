//! Settings commands for the MCP server (see `crate::mcp`).

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::mcp::McpStatus;
use crate::state::AppState;

#[tauri::command]
pub async fn get_mcp_status() -> Result<McpStatus, AppError> {
    Ok(crate::mcp::status().await)
}

#[tauri::command]
pub async fn set_mcp_settings(
    state: State<'_, Arc<AppState>>,
    enabled: bool,
    port: u16,
    tunnel: Option<bool>,
    public_url: Option<String>,
) -> Result<McpStatus, AppError> {
    let update = crate::mcp::SettingsUpdate {
        enabled,
        port,
        tunnel,
        public_url,
    };
    Ok(crate::mcp::set_settings(state.inner().clone(), update).await?)
}

#[tauri::command]
pub async fn regenerate_mcp_token() -> Result<McpStatus, AppError> {
    Ok(crate::mcp::regenerate_token().await?)
}
