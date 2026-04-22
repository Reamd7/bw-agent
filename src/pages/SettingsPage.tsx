import { createSignal, onMount, Show } from "solid-js";
import {
  getConfig,
  saveConfig,
  lockVault,
  updateLockMode,
  getGitSigningStatus,
  configureGitSigning,
  type Config,
  type LockMode,
  type GitSigningStatus,
} from "../lib/tauri";
import { store } from "../lib/store";

function navigate(path: string) {
  window.location.hash = "#" + path;
}

function goBack() {
  navigate(store.locked ? "/" : "/dashboard");
}

export default function SettingsPage() {
  const [config, setConfig] = createSignal<Config>({
    email: "",
    base_url: "",
    identity_url: "",
    lock_mode: { type: "timeout", seconds: 900 },
    proxy: "",
  });
  const [loading, setLoading] = createSignal(true);
  const [saving, setSaving] = createSignal(false);
  const [toast, setToast] = createSignal<{ message: string; type: "success" | "error" } | null>(null);
  let originalConfig: Config | null = null;
  const [gitSigningStatus, setGitSigningStatus] = createSignal<GitSigningStatus | null>(null);
  const [configuring, setConfiguring] = createSignal(false);

  const [lockPreset, setLockPreset] = createSignal<string>("15m");

  onMount(async () => {
    try {
      const currentConfig = await getConfig();
      setConfig(currentConfig);
      originalConfig = { ...currentConfig };
      
      const mode = currentConfig.lock_mode;
      if (mode.type === "timeout") {
        if (mode.seconds === 60) setLockPreset("1m");
        else if (mode.seconds === 300) setLockPreset("5m");
        else if (mode.seconds === 900) setLockPreset("15m");
        else if (mode.seconds === 1800) setLockPreset("30m");
        else if (mode.seconds === 3600) setLockPreset("1h");
        else if (mode.seconds === 14400) setLockPreset("4h");
        else setLockPreset("custom");
      } else if (mode.type === "system_idle") {
        setLockPreset("idle");
      } else if (mode.type === "on_sleep") {
        setLockPreset("sleep");
      } else if (mode.type === "on_lock") {
        setLockPreset("lock");
      } else if (mode.type === "on_restart") {
        setLockPreset("restart");
      } else if (mode.type === "never") {
        setLockPreset("never");
      }

      try {
        const status = await getGitSigningStatus();
        setGitSigningStatus(status);
      } catch (e) {
        console.error("Failed to get git signing status:", e);
      }
    } catch (e) {
      console.error("Failed to load config:", e);
      setToast({ message: "Failed to load settings", type: "error" });
    } finally {
      setLoading(false);
    }
  });

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    setSaving(true);
    setToast(null);

    try {
      const newConfig = config();
      await saveConfig(newConfig);
      await updateLockMode(newConfig.lock_mode);

      const emailChanged = originalConfig && originalConfig.email !== newConfig.email;
      const baseUrlChanged = originalConfig && originalConfig.base_url !== newConfig.base_url;
      originalConfig = { ...newConfig };

      if (emailChanged || baseUrlChanged) {
        await lockVault();
        navigate("/");
        return;
      }

      setToast({ message: "Settings saved successfully", type: "success" });
      setTimeout(() => setToast(null), 3000);
    } catch (e) {
      console.error("Failed to save config:", e);
      setToast({ message: "Failed to save settings", type: "error" });
    } finally {
      setSaving(false);
    }
  };

  const handlePresetChange = (preset: string) => {
    setLockPreset(preset);
    let newMode: LockMode;
    const currentMode = config().lock_mode;
    
    switch (preset) {
      case "1m": newMode = { type: "timeout", seconds: 60 }; break;
      case "5m": newMode = { type: "timeout", seconds: 300 }; break;
      case "15m": newMode = { type: "timeout", seconds: 900 }; break;
      case "30m": newMode = { type: "timeout", seconds: 1800 }; break;
      case "1h": newMode = { type: "timeout", seconds: 3600 }; break;
      case "4h": newMode = { type: "timeout", seconds: 14400 }; break;
      case "custom": 
        newMode = { type: "timeout", seconds: currentMode.type === "timeout" ? currentMode.seconds : 900 }; 
        break;
      case "idle": 
        newMode = { type: "system_idle", seconds: currentMode.type === "system_idle" ? currentMode.seconds : 300 }; 
        break;
      case "sleep": newMode = { type: "on_sleep" }; break;
      case "lock": newMode = { type: "on_lock" }; break;
      case "restart": newMode = { type: "on_restart" }; break;
      case "never": newMode = { type: "never" }; break;
      default: newMode = { type: "timeout", seconds: 900 };
    }
    
    setConfig(prev => ({ ...prev, lock_mode: newMode }));
  };

  const handleConfigureGitSigning = async () => {
    setConfiguring(true);
    setToast(null);

    try {
      await configureGitSigning();
      const status = await getGitSigningStatus();
      setGitSigningStatus(status);
      setToast({ message: "Git SSH signing configured successfully", type: "success" });
      setTimeout(() => setToast(null), 3000);
    } catch (e) {
      console.error("Failed to configure git signing:", e);
      setToast({ message: "Failed to configure git signing", type: "error" });
    } finally {
      setConfiguring(false);
    }
  };

  const allCorrect = () =>
    gitSigningStatus()?.program_correct &&
    gitSigningStatus()?.format_correct &&
    gitSigningStatus()?.signing_enabled;

  const signChecks = () => (
    <ul class="mt-2 space-y-1 text-sm">
      <li class="flex items-center gap-2">
        <span class={gitSigningStatus()?.program_correct ? "text-[var(--success)]" : "text-[var(--danger)]"}>
          {gitSigningStatus()?.program_correct ? "\u2705" : "\u274C"}
        </span>
        <span style={`color: var(--text-primary)`}>gpg.ssh.program</span>
        <Show when={gitSigningStatus()?.program_correct && gitSigningStatus()?.ssh_program}>
          <span class="text-xs break-all" style={`color: var(--text-tertiary)`}>({gitSigningStatus()?.ssh_program})</span>
        </Show>
        <Show when={!gitSigningStatus()?.program_correct && gitSigningStatus()?.ssh_program != null}>
          <span class="text-xs opacity-75 break-all" style={`color: var(--text-tertiary)`}>(current: {gitSigningStatus()?.ssh_program})</span>
        </Show>
      </li>
      <li class="flex items-center gap-2">
        <span class={gitSigningStatus()?.format_correct ? "text-[var(--success)]" : "text-[var(--danger)]"}>
          {gitSigningStatus()?.format_correct ? "\u2705" : "\u274C"}
        </span>
        <span style={`color: var(--text-primary)`}>gpg.format = ssh</span>
      </li>
      <li class="flex items-center gap-2">
        <span class={gitSigningStatus()?.signing_enabled ? "text-[var(--success)]" : "text-[var(--danger)]"}>
          {gitSigningStatus()?.signing_enabled ? "\u2705" : "\u274C"}
        </span>
        <span style={`color: var(--text-primary)`}>commit.gpgsign = true</span>
      </li>
    </ul>
  );

  const updateField = (field: keyof Config, value: string | number | null) => {
    setConfig((prev) => ({ ...prev, [field]: value }));
  };

  return (
    <div class="min-h-screen" style={`background: var(--bg-secondary)`}>
      <div class="mx-auto max-w-2xl px-4 py-8">
        {/* Header */}
        <div class="mb-6 flex items-center gap-3">
          <button
            onClick={goBack}
            class="btn-ghost"
            style={{ "border-radius": "var(--radius-md)", padding: "8px" }}
          >
            <svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
              <path stroke-linecap="round" stroke-linejoin="round" d="M10.5 19.5L3 12m0 0l7.5-7.5M3 12h18" />
            </svg>
          </button>
          <h1 class="text-lg font-semibold" style={`color: var(--text-primary)`}>Settings</h1>
        </div>

        {/* Toast */}
        <Show when={toast()}>
          {(t) => (
            <div class={`toast mb-4 ${t().type === "success" ? "toast-success" : "toast-error"}`}>
              <svg class="h-4 w-4 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                {t().type === "success"
                  ? <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                  : <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m9-.75a9 9 0 11-18 0 9 9 0 0118 0zm-9 3.75h.008v.008H12v-.008z" />
                }
              </svg>
              {t().message}
            </div>
          )}
        </Show>

        <Show
          when={!loading()}
          fallback={
            <div class="flex items-center justify-center py-20">
              <svg class="w-6 h-6 animate-spin" style={`color: var(--text-tertiary)`} fill="none" viewBox="0 0 24 24">
                <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
                <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
            </div>
          }
        >
          <form onSubmit={handleSubmit} class="space-y-4">
            {/* Account Section */}
            <div class="card" style={{ padding: "24px" }}>
              <div class="flex items-center gap-2 mb-4">
                <svg class="w-4 h-4" style={`color: var(--text-tertiary)`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M15.75 6a3.75 3.75 0 11-7.5 0 3.75 3.75 0 017.5 0zM4.501 20.118a7.5 7.5 0 0114.998 0A17.933 17.933 0 0112 21.75c-2.676 0-5.216-.584-7.499-1.632z" />
                </svg>
                <h2 class="text-sm font-semibold" style={`color: var(--text-primary)`}>Account</h2>
              </div>
              <p class="text-xs mb-4" style={`color: var(--text-tertiary)`}>Your Bitwarden account details.</p>
              <div>
                <label for="email" class="block text-xs font-medium mb-1.5" style={`color: var(--text-secondary)`}>
                  Email Address
                </label>
                <input
                  type="email"
                  id="email"
                  value={config().email || ""}
                  onInput={(e) => updateField("email", e.currentTarget.value || null)}
                  class="input"
                  placeholder="you@example.com"
                />
              </div>
            </div>

            {/* Server Configuration */}
            <div class="card" style={{ padding: "24px" }}>
              <div class="flex items-center gap-2 mb-4">
                <svg class="w-4 h-4" style={`color: var(--text-tertiary)`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M5.25 14.25h13.5m-13.5 0a3 3 0 01-3-3m3 3a3 3 0 100 6h13.5a3 3 0 100-6m-16.5-3a3 3 0 013-3h13.5a3 3 0 013 3m-19.5 0a4.5 4.5 0 01.9-2.7L5.737 5.1a3.375 3.375 0 012.7-1.35h7.126c1.062 0 2.062.5 2.7 1.35l2.587 3.45a4.5 4.5 0 01.9 2.7m0 0a3 3 0 01-3 3m0 3h.008v.008h-.008v-.008zm0-6h.008v.008h-.008v-.008zm-3 6h.008v.008h-.008v-.008zm0-6h.008v.008h-.008v-.008z" />
                </svg>
                <h2 class="text-sm font-semibold" style={`color: var(--text-primary)`}>Server</h2>
              </div>
              <p class="text-xs mb-4" style={`color: var(--text-tertiary)`}>Configure your self-hosted Bitwarden server.</p>
              <div>
                <label for="base_url" class="block text-xs font-medium mb-1.5" style={`color: var(--text-secondary)`}>
                  Server URL
                </label>
                <input
                  type="url"
                  id="base_url"
                  value={config().base_url || ""}
                  onInput={(e) => updateField("base_url", e.currentTarget.value || null)}
                  class="input"
                  placeholder="https://bitwarden.example.com"
                />
              </div>
            </div>

            {/* Security */}
            <div class="card" style={{ padding: "24px" }}>
              <div class="flex items-center gap-2 mb-4">
                <svg class="w-4 h-4" style={`color: var(--text-tertiary)`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M9 12.75L11.25 15 15 9.75m-3-7.036A11.959 11.959 0 013.598 6 11.99 11.99 0 003 9.749c0 5.592 3.824 10.29 9 11.623 5.176-1.332 9-6.03 9-11.622 0-1.31-.21-2.571-.598-3.751h-.152c-3.196 0-6.1-1.248-8.25-3.285z" />
                </svg>
                <h2 class="text-sm font-semibold" style={`color: var(--text-primary)`}>Security</h2>
              </div>
              <p class="text-xs mb-4" style={`color: var(--text-tertiary)`}>Manage how the agent secures your keys.</p>
              <div>
                <label for="lock_mode" class="block text-xs font-medium mb-1.5" style={`color: var(--text-secondary)`}>
                  Vault Timeout
                </label>
                <select
                  id="lock_mode"
                  value={lockPreset()}
                  onChange={(e) => handlePresetChange(e.currentTarget.value)}
                  class="input"
                  style={{ "padding-right": "36px", appearance: "none", backgroundImage: "url(\"data:image/svg+xml,%3csvg xmlns='http://www.w3.org/2000/svg' fill='none' viewBox='0 0 20 20'%3e%3cpath stroke='%236b7280' stroke-linecap='round' stroke-linejoin='round' stroke-width='1.5' d='M6 8l4 4 4-4'/%3e%3c/svg%3e\")", backgroundPosition: "right 8px center", backgroundRepeat: "no-repeat", backgroundSize: "20px" }}
                >
                  <optgroup label="Time-based">
                    <option value="1m">1 Minute</option>
                    <option value="5m">5 Minutes</option>
                    <option value="15m">15 Minutes</option>
                    <option value="30m">30 Minutes</option>
                    <option value="1h">1 Hour</option>
                    <option value="4h">4 Hours</option>
                    <option value="custom">Custom...</option>
                  </optgroup>
                  <optgroup label="System Events">
                    <option value="idle">On System Idle</option>
                    <option value="sleep">On System Sleep</option>
                    <option value="lock">On Screen Lock</option>
                    <option value="restart">On Restart</option>
                  </optgroup>
                  <option value="never">Never</option>
                </select>
                
                <Show when={lockPreset() === "custom"}>
                  <div class="mt-2 flex items-center gap-2">
                    <input
                      type="number"
                      id="custom_seconds"
                      min="1"
                      value={config().lock_mode.type === "timeout" ? (config().lock_mode as any).seconds : 900}
                      onInput={(e) => {
                        const val = parseInt(e.currentTarget.value) || 0;
                        setConfig(prev => ({ ...prev, lock_mode: { type: "timeout", seconds: val } }));
                      }}
                      class="input"
                      style={{ width: "120px" }}
                      placeholder="Seconds"
                    />
                    <span class="text-sm" style={`color: var(--text-tertiary)`}>seconds</span>
                  </div>
                </Show>

                <Show when={lockPreset() === "idle"}>
                  <div class="mt-2 flex items-center gap-2">
                    <input
                      type="number"
                      id="idle_seconds"
                      min="1"
                      value={config().lock_mode.type === "system_idle" ? (config().lock_mode as any).seconds : 300}
                      onInput={(e) => {
                        const val = parseInt(e.currentTarget.value) || 0;
                        setConfig(prev => ({ ...prev, lock_mode: { type: "system_idle", seconds: val } }));
                      }}
                      class="input"
                      style={{ width: "120px" }}
                      placeholder="Seconds"
                    />
                    <span class="text-sm" style={`color: var(--text-tertiary)`}>seconds</span>
                  </div>
                </Show>
              </div>
            </div>

            {/* Network */}
            <div class="card" style={{ padding: "24px" }}>
              <div class="flex items-center gap-2 mb-4">
                <svg class="w-4 h-4" style={`color: var(--text-tertiary)`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M12 21a9.004 9.004 0 008.716-6.747M12 21a9.004 9.004 0 01-8.716-6.747M12 21c2.485 0 4.5-4.03 4.5-9S14.485 3 12 3m0 18c-2.485 0-4.5-4.03-4.5-9S9.515 3 12 3m0 0a8.997 8.997 0 017.843 4.582M12 3a8.997 8.997 0 00-7.843 4.582m15.686 0A11.953 11.953 0 0112 10.5c-2.998 0-5.74-1.1-7.843-2.918m15.686 0A8.959 8.959 0 0121 12c0 .778-.099 1.533-.284 2.253m0 0A17.919 17.919 0 0112 16.5c-3.162 0-6.133-.815-8.716-2.247m0 0A9.015 9.015 0 013 12c0-1.605.42-3.113 1.157-4.418" />
                </svg>
                <h2 class="text-sm font-semibold" style={`color: var(--text-primary)`}>Network</h2>
              </div>
              <div>
                <label for="proxy" class="block text-xs font-medium mb-1.5" style={`color: var(--text-secondary)`}>
                  Proxy URL
                </label>
                <input
                  type="text"
                  id="proxy"
                  value={config().proxy || ""}
                  onInput={(e) => updateField("proxy", e.currentTarget.value || null)}
                  class="input"
                  placeholder="http://proxy.example.com:8080"
                />
              </div>
            </div>

            {/* Git Signing */}
            <div class="card" style={{ padding: "24px" }}>
              <div class="flex items-center gap-2 mb-4">
                <svg class="w-4 h-4" style={`color: var(--text-tertiary)`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="1.5">
                  <path stroke-linecap="round" stroke-linejoin="round" d="M17.25 6.75L22.5 12l-5.25 5.25m-10.5 0L1.5 12l5.25-5.25m7.5-3l-4.5 16.5" />
                </svg>
                <h2 class="text-sm font-semibold" style={`color: var(--text-primary)`}>Git Signing</h2>
              </div>
              <p class="text-xs mb-4" style={`color: var(--text-tertiary)`}>
                Configure git to use bw-agent for SSH commit signing.
              </p>
              <Show
                when={allCorrect()}
                fallback={
                  <div class="rounded-lg p-4" style={`background: var(--warning-bg); border: 1px solid #fde68a`}>
                    <div class="flex items-start gap-3">
                      <svg class="h-4 w-4 mt-0.5 shrink-0" style={`color: var(--warning)`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                        <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z" />
                      </svg>
                      <div class="flex-1">
                        <p class="text-sm font-medium" style={`color: var(--warning-text)`}>Git SSH signing is not fully configured</p>
                        {signChecks()}
                        <div class="mt-3">
                          <button
                            type="button"
                            onClick={handleConfigureGitSigning}
                            disabled={configuring()}
                            class="btn btn-secondary text-xs"
                            style={{ "border-color": "var(--warning)", color: "var(--warning-text)" }}
                          >
                            {configuring() ? "Configuring..." : "Configure Git SSH Signing"}
                          </button>
                        </div>
                      </div>
                    </div>
                  </div>
                }
              >
                <div class="rounded-lg p-4" style={`background: var(--success-bg); border: 1px solid #a7f3d0`}>
                  <div class="flex items-start gap-3">
                    <svg class="h-4 w-4 mt-0.5 shrink-0" style={`color: var(--success)`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                      <path stroke-linecap="round" stroke-linejoin="round" d="M4.5 12.75l6 6 9-13.5" />
                    </svg>
                    <div>
                      <p class="text-sm font-medium" style={`color: var(--success-text)`}>Git SSH signing is configured</p>
                      {signChecks()}
                    </div>
                  </div>
                </div>
              </Show>
            </div>

            {/* Save */}
            <div class="flex justify-end pt-2 pb-8">
              <button
                type="submit"
                disabled={saving()}
                class="btn btn-primary"
              >
                {saving() ? "Saving..." : "Save Settings"}
              </button>
            </div>
          </form>
        </Show>
      </div>
    </div>
  );
}
