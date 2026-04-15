import { invoke } from "@tauri-apps/api/core";

export interface SshKeyInfo {
  name: string;
  key_type: string;
  fingerprint: string;
}

export type UnlockResult = 
  | "Success"
  | { TwoFactorRequired: { providers: number[] } };

export interface AccessLogEntry {
  id: number;
  timestamp: string;
  key_fingerprint: string;
  key_name: string;
  client_exe: string;
  client_pid: number;
  approved: boolean;
}

export interface ApprovalRequest {
  id: string;
  key_name: string;
  key_fingerprint: string;
  client_exe: string;
  client_pid: number;
  timestamp: number;
}

export interface Config {
  email: string | null;
  base_url: string | null;
  identity_url: string | null;
  lock_timeout: number;
  proxy: string | null;
}

export const unlock = (password: string) => invoke<UnlockResult>("unlock", { password });
export const submitPassword = (password: string | null) => invoke<void>("submit_password", { password });
export const submitTwoFactor = (provider: number, code: string) => invoke<void>("submit_two_factor", { provider, code });
export const listKeys = () => invoke<SshKeyInfo[]>("list_keys");
export const getAccessLogs = (limit: number) => invoke<AccessLogEntry[]>("get_access_logs", { limit });
export const approveRequest = (request_id: string, approved: boolean) => invoke<void>("approve_request", { request_id, approved });
export const getPendingApprovals = () => invoke<ApprovalRequest[]>("get_pending_approvals");
export const lockVault = () => invoke<void>("lock_vault");
export const getConfig = () => invoke<Config>("get_config");
export const saveConfig = (config: Config) => invoke<void>("save_config", { config });
