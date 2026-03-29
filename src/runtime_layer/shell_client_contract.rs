use alloc::string::String;

use super::super::gui::protocol::{
    DesktopPermission, PermissionState, SessionPowerState, SessionSnapshot, WorkspaceId,
    WorkspaceLayout, WorkspaceRule,
};
pub use super::super::services::{ShellCommand, ShellResponse};

#[inline]
pub fn request_shell_sync(app_id: u32, command: ShellCommand) -> Option<ShellResponse> {
    super::super::ipc::request_shell_sync(app_id, command)
}

pub fn register_shell_app(app_id: u32, name: &str) -> Result<(), String> {
    match request_shell_sync(
        app_id,
        ShellCommand::RegisterApp {
            app_id,
            name: String::from(name),
        },
    ) {
        Some(ShellResponse::Ack) => Ok(()),
        Some(ShellResponse::Error(err)) => Err(err),
        _ => Err(String::from("shell service unavailable")),
    }
}

pub fn set_permission(
    app_id: u32,
    permission: DesktopPermission,
    state: PermissionState,
) -> Result<(), String> {
    match request_shell_sync(
        app_id,
        ShellCommand::SetPermission {
            app_id,
            permission,
            state,
        },
    ) {
        Some(ShellResponse::Ack) => Ok(()),
        Some(ShellResponse::Error(err)) => Err(err),
        _ => Err(String::from("shell service unavailable")),
    }
}

pub fn grant_file_access(app_id: u32, path_prefix: &str, read_only: bool) -> Result<(), String> {
    match request_shell_sync(
        app_id,
        ShellCommand::GrantFileAccess {
            app_id,
            path_prefix: String::from(path_prefix),
            read_only,
        },
    ) {
        Some(ShellResponse::Ack) => Ok(()),
        Some(ShellResponse::Error(err)) => Err(err),
        _ => Err(String::from("shell service unavailable")),
    }
}

pub fn set_workspace_layout(
    workspace_id: WorkspaceId,
    layout: WorkspaceLayout,
) -> Result<(), String> {
    match request_shell_sync(
        0,
        ShellCommand::SetWorkspaceLayout {
            workspace_id,
            layout,
        },
    ) {
        Some(ShellResponse::Ack) => Ok(()),
        Some(ShellResponse::Error(err)) => Err(err),
        _ => Err(String::from("shell service unavailable")),
    }
}

pub fn set_workspace_rule(workspace_id: WorkspaceId, rule: WorkspaceRule) -> Result<(), String> {
    match request_shell_sync(0, ShellCommand::SetWorkspaceRule { workspace_id, rule }) {
        Some(ShellResponse::Ack) => Ok(()),
        Some(ShellResponse::Error(err)) => Err(err),
        _ => Err(String::from("shell service unavailable")),
    }
}

pub fn set_power_state(power_state: SessionPowerState) -> Result<(), String> {
    match request_shell_sync(0, ShellCommand::SetPowerState { power_state }) {
        Some(ShellResponse::Ack) => Ok(()),
        Some(ShellResponse::Error(err)) => Err(err),
        _ => Err(String::from("shell service unavailable")),
    }
}

pub fn session_snapshot() -> Result<SessionSnapshot, String> {
    match request_shell_sync(0, ShellCommand::GetSessionSnapshot) {
        Some(ShellResponse::SessionSnapshot(snapshot)) => Ok(snapshot),
        Some(ShellResponse::Error(err)) => Err(err),
        _ => Err(String::from("shell service unavailable")),
    }
}
