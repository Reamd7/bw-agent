import { invoke } from "@tauri-apps/api/core";

export interface CustomFieldInfo {
  name: string;
  value: string;
  field_type: number;
}

export interface CustomFieldInput {
  name: string;
  value: string;
  field_type: number;
}

export interface SshKeyInfo {
  entry_id: string;
  name: string;
  key_type: string;
  fingerprint: string;
  public_key: string;
  match_patterns: string[];
  custom_fields: CustomFieldInfo[];
}

export type UnlockResult = 
  | "Success"
  | { TwoFactorRequired: { providers: number[] } };

export interface ProcessInfo {
  exe: string;
  pid: number;
  cmdline: string;
}

export interface AccessLogEntry {
  id: number;
  timestamp: string;
  key_fingerprint: string;
  key_name: string;
  client_exe: string;
  client_pid: number;
  process_chain: ProcessInfo[];
  approved: boolean;
  auto_approved: boolean;
  session_id: string | null;
}

export interface ApprovalRequest {
  id: string;
  key_name: string;
  key_fingerprint: string;
  client_exe: string;
  client_pid: number;
  process_chain: ProcessInfo[];
  timestamp: number;
}

export interface SessionScope {
  type: "any_process" | "executable";
  exe_path?: string;
  exe_hash?: number[];
}

export interface ApprovalSessionInfo {
  id: string;
  key_fingerprint: string;
  scope: SessionScope;
  created_at_unix: number;
  expires_at_unix: number;
  remaining_secs: number;
  usage_count: number;
}

export type LockMode =
  | { type: "timeout"; seconds: number }
  | { type: "system_idle"; seconds: number }
  | { type: "on_sleep" }
  | { type: "on_lock" }
  | { type: "on_restart" }
  | { type: "never" };

export interface Config {
  email: string | null;
  base_url: string | null;
  identity_url: string | null;
  lock_mode: LockMode;
  proxy: string | null;
}

export interface GitSigningStatus {
  ssh_program: string | null;
  gpg_format: string | null;
  commit_gpgsign: boolean;
  /** Whether gpg.ssh.program points to our bw-agent binary */
  program_correct: boolean;
  /** Whether gpg.format == "ssh" */
  format_correct: boolean;
  /** Whether commit.gpgsign == true */
  signing_enabled: boolean;
}

export const unlock = (password: string) => invoke<UnlockResult>("unlock", { password });
export const submitPassword = (password: string | null) => invoke<void>("submit_password", { password });
export const submitTwoFactor = (provider: number, code: string) => invoke<void>("submit_two_factor", { provider, code });
export const unlockWithTwoFactor = (provider: number, code: string, remember: boolean) =>
  invoke<UnlockResult>("unlock_with_two_factor", { provider, code, remember });
export const listKeys = () => invoke<SshKeyInfo[]>("list_keys");
export const getAccessLogs = (limit: number) => invoke<AccessLogEntry[]>("get_access_logs", { limit });
export const approveRequest = (request_id: string, approved: boolean) => invoke<void>("approve_request", { requestId: request_id, approved });
export const getPendingApprovals = () => invoke<ApprovalRequest[]>("get_pending_approvals");
export const lockVault = () => invoke<void>("lock_vault");
export const manualSync = () => invoke<void>("manual_sync");
export const getConfig = () => invoke<Config>("get_config");
export const saveConfig = (config: Config) => invoke<void>("save_config", { config });
export const updateLockMode = (lockMode: LockMode) => invoke<void>("update_lock_mode", { lockMode });
export const getGitSigningStatus = () => invoke<GitSigningStatus>("get_git_signing_status");
export const configureGitSigning = () => invoke<void>("configure_git_signing");
export const getGitSignProgramPath = () => invoke<string>("get_git_sign_program_path");
export const updateKeyFields = (entryId: string, fields: CustomFieldInput[]) =>
  invoke<void>("update_key_fields", { entryId, fields });

export interface AuthRequestResult {
  request_id: string;
  fingerprint: string;
}
export interface PollAuthRequestResult {
  approved: boolean;
  fingerprint_validated: boolean;
  two_factor_required: number[] | null;
}
export const createAuthRequest = () => invoke<AuthRequestResult>("create_auth_request");
export const pollAuthRequest = () => invoke<PollAuthRequestResult>("poll_auth_request");
export const cancelAuthRequest = () => invoke<void>("cancel_auth_request");
export const submitAuthRequestTwoFactor = (provider: number, code: string, remember: boolean) =>
  invoke<{ success: boolean }>("submit_auth_request_two_factor", { provider, code, remember });

export const approveRequestWithSession = (
  requestId: string,
  durationSecs: number,
  scopeType: string,
  scopeExePath?: string,
) => invoke<void>("approve_request_with_session", {
  requestId,
  durationSecs,
  scopeType,
  scopeExePath,
});

export const listActiveSessions = () =>
  invoke<ApprovalSessionInfo[]>("list_active_sessions");

export const revokeSession = (sessionId: string) =>
  invoke<boolean>("revoke_session", { sessionId });

export const hasTwoFactorRemember = () => invoke<boolean>("has_two_factor_remember");
export const revokeTwoFactorRemember = () => invoke<void>("revoke_two_factor_remember");
export const hasRegisteredDevice = () => invoke<boolean>("has_registered_device");
