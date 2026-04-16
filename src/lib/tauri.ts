import { invoke } from "@tauri-apps/api/core";

export interface SshKeyInfo {
  name: string;
  key_type: string;
  fingerprint: string;
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

export const unlock = (password: string) => invoke<UnlockResult>("unlock", { password });
export const submitPassword = (password: string | null) => invoke<void>("submit_password", { password });
export const submitTwoFactor = (provider: number, code: string) => invoke<void>("submit_two_factor", { provider, code });
export const listKeys = () => invoke<SshKeyInfo[]>("list_keys");
export const getAccessLogs = (limit: number) => invoke<AccessLogEntry[]>("get_access_logs", { limit });
export const approveRequest = (request_id: string, approved: boolean) => invoke<void>("approve_request", { requestId: request_id, approved });
export const getPendingApprovals = () => invoke<ApprovalRequest[]>("get_pending_approvals");
export const lockVault = () => invoke<void>("lock_vault");
export const getConfig = () => invoke<Config>("get_config");
export const saveConfig = (config: Config) => invoke<void>("save_config", { config });
export const updateLockMode = (lockMode: LockMode) => invoke<void>("update_lock_mode", { lockMode });
