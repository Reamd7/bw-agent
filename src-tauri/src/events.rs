use tauri::{AppHandle, Emitter};

pub const EVENT_PASSWORD_REQUESTED: &str = "password-requested";
pub const EVENT_TWO_FACTOR_REQUESTED: &str = "two-factor-requested";
pub const EVENT_APPROVAL_REQUESTED: &str = "approval-requested";
pub const EVENT_LOCK_STATE_CHANGED: &str = "lock-state-changed";
pub const EVENT_VAULT_SYNCED: &str = "vault-synced";

#[derive(serde::Serialize, Clone)]
pub struct PasswordRequestPayload {
    pub email: String,
    pub error: Option<String>,
}

#[derive(serde::Serialize, Clone)]
pub struct TwoFactorRequestPayload {
    pub providers: Vec<u8>,
}

#[derive(serde::Serialize, Clone)]
pub struct LockStatePayload {
    pub locked: bool,
}

pub fn emit_password_requested(
    app_handle: &AppHandle,
    payload: PasswordRequestPayload,
) -> tauri::Result<()> {
    app_handle.emit(EVENT_PASSWORD_REQUESTED, payload)
}

pub fn emit_two_factor_requested(
    app_handle: &AppHandle,
    payload: TwoFactorRequestPayload,
) -> tauri::Result<()> {
    app_handle.emit(EVENT_TWO_FACTOR_REQUESTED, payload)
}

pub fn emit_approval_requested(
    app_handle: &AppHandle,
    payload: bw_agent::ApprovalRequest,
) -> tauri::Result<()> {
    app_handle.emit(EVENT_APPROVAL_REQUESTED, payload)
}

#[derive(serde::Serialize, Clone)]
pub struct VaultSyncedPayload {
    pub success: bool,
    pub error: Option<String>,
}

pub fn emit_lock_state_changed(app_handle: &AppHandle, locked: bool) -> tauri::Result<()> {
    app_handle.emit(EVENT_LOCK_STATE_CHANGED, LockStatePayload { locked })
}

pub fn emit_vault_synced(app_handle: &AppHandle, payload: VaultSyncedPayload) -> tauri::Result<()> {
    app_handle.emit(EVENT_VAULT_SYNCED, payload)
}
