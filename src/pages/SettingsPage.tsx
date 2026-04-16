import { createSignal, onMount, Show } from "solid-js";
import { getConfig, saveConfig, lockVault, updateLockMode, type Config, type LockMode } from "../lib/tauri";
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

  const updateField = (field: keyof Config, value: string | number | null) => {
    setConfig((prev) => ({ ...prev, [field]: value }));
  };

  return (
    <div class="min-h-screen bg-gray-50 py-10">
      <div class="mx-auto max-w-3xl px-4 sm:px-6 lg:px-8">
        <div class="mb-8 flex items-center justify-between">
          <div class="flex items-center">
            <button
              onClick={goBack}
              class="mr-4 rounded-full p-2 text-gray-400 hover:bg-gray-200 hover:text-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
            >
              <svg class="h-6 w-6" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 19l-7-7m0 0l7-7m-7 7h18" />
              </svg>
            </button>
            <h1 class="text-2xl font-bold text-gray-900">Settings</h1>
          </div>
        </div>

        <Show when={toast()}>
          {(t) => (
            <div
              class={`mb-6 rounded-md p-4 ${
                t().type === "success" ? "bg-green-50" : "bg-red-50"
              }`}
            >
              <div class="flex">
                <div class="flex-shrink-0">
                  {t().type === "success" ? (
                    <svg class="h-5 w-5 text-green-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                    </svg>
                  ) : (
                    <svg class="h-5 w-5 text-red-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" />
                    </svg>
                  )}
                </div>
                <div class="ml-3">
                  <p
                    class={`text-sm font-medium ${
                      t().type === "success" ? "text-green-800" : "text-red-800"
                    }`}
                  >
                    {t().message}
                  </p>
                </div>
              </div>
            </div>
          )}
        </Show>

        <div class="overflow-hidden rounded-lg bg-white shadow">
          <Show
            when={!loading()}
            fallback={<div class="p-8 text-center text-gray-500">Loading settings...</div>}
          >
            <form onSubmit={handleSubmit} class="divide-y divide-gray-200">
              <div class="p-6 sm:p-8">
                <div class="grid grid-cols-1 gap-y-6 sm:grid-cols-6 sm:gap-x-6">
                  <div class="sm:col-span-6">
                    <h2 class="text-lg font-medium leading-6 text-gray-900">Account</h2>
                    <p class="mt-1 text-sm text-gray-500">
                      Your Bitwarden account details.
                    </p>
                  </div>

                  <div class="sm:col-span-4">
                    <label for="email" class="block text-sm font-medium text-gray-700">
                      Email Address
                    </label>
                    <div class="mt-1">
                      <input
                        type="email"
                        id="email"
                        value={config().email || ""}
                        onInput={(e) => updateField("email", e.currentTarget.value || null)}
                        class="block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 sm:text-sm px-3 py-2 border"
                        placeholder="you@example.com"
                      />
                    </div>
                  </div>

                  <div class="sm:col-span-6 pt-6">
                    <h2 class="text-lg font-medium leading-6 text-gray-900">Server Configuration</h2>
                    <p class="mt-1 text-sm text-gray-500">
                      Configure your self-hosted Bitwarden server if applicable.
                    </p>
                  </div>

                  <div class="sm:col-span-6">
                    <label for="base_url" class="block text-sm font-medium text-gray-700">
                      Server URL
                    </label>
                    <div class="mt-1">
                      <input
                        type="url"
                        id="base_url"
                        value={config().base_url || ""}
                        onInput={(e) => updateField("base_url", e.currentTarget.value || null)}
                        class="block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 sm:text-sm px-3 py-2 border"
                        placeholder="https://bitwarden.example.com"
                      />
                    </div>
                  </div>

                  <div class="sm:col-span-6 pt-6">
                    <h2 class="text-lg font-medium leading-6 text-gray-900">Security</h2>
                    <p class="mt-1 text-sm text-gray-500">
                      Manage how the agent secures your keys.
                    </p>
                  </div>

                  <div class="sm:col-span-3">
                    <label for="lock_mode" class="block text-sm font-medium text-gray-700">
                      Vault Timeout
                    </label>
                    <div class="mt-1">
                      <select
                        id="lock_mode"
                        value={lockPreset()}
                        onChange={(e) => handlePresetChange(e.currentTarget.value)}
                        class="block w-full rounded-md border-gray-300 bg-white shadow-sm focus:border-blue-500 focus:ring-blue-500 sm:text-sm px-3 py-2 border"
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
                    </div>
                    
                    <Show when={lockPreset() === "custom"}>
                      <div class="mt-2">
                        <label for="custom_seconds" class="sr-only">Seconds</label>
                        <div class="flex items-center">
                          <input
                            type="number"
                            id="custom_seconds"
                            min="1"
                            value={config().lock_mode.type === "timeout" ? (config().lock_mode as any).seconds : 900}
                            onInput={(e) => {
                              const val = parseInt(e.currentTarget.value) || 0;
                              setConfig(prev => ({ ...prev, lock_mode: { type: "timeout", seconds: val } }));
                            }}
                            class="block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 sm:text-sm px-3 py-2 border"
                            placeholder="Seconds"
                          />
                          <span class="ml-2 text-sm text-gray-500">seconds</span>
                        </div>
                      </div>
                    </Show>

                    <Show when={lockPreset() === "idle"}>
                      <div class="mt-2">
                        <label for="idle_seconds" class="sr-only">Idle Duration (seconds)</label>
                        <div class="flex items-center">
                          <input
                            type="number"
                            id="idle_seconds"
                            min="1"
                            value={config().lock_mode.type === "system_idle" ? (config().lock_mode as any).seconds : 300}
                            onInput={(e) => {
                              const val = parseInt(e.currentTarget.value) || 0;
                              setConfig(prev => ({ ...prev, lock_mode: { type: "system_idle", seconds: val } }));
                            }}
                            class="block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 sm:text-sm px-3 py-2 border"
                            placeholder="Seconds"
                          />
                          <span class="ml-2 text-sm text-gray-500">seconds</span>
                        </div>
                      </div>
                    </Show>
                  </div>

                  <div class="sm:col-span-6 pt-6">
                    <h2 class="text-lg font-medium leading-6 text-gray-900">Network</h2>
                  </div>

                  <div class="sm:col-span-6">
                    <label for="proxy" class="block text-sm font-medium text-gray-700">
                      Proxy URL
                    </label>
                    <div class="mt-1">
                      <input
                        type="text"
                        id="proxy"
                        value={config().proxy || ""}
                        onInput={(e) => updateField("proxy", e.currentTarget.value || null)}
                        class="block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 sm:text-sm px-3 py-2 border"
                        placeholder="http://proxy.example.com:8080"
                      />
                    </div>
                  </div>
                </div>
              </div>
              <div class="bg-gray-50 px-4 py-3 text-right sm:px-6">
                <button
                  type="submit"
                  disabled={saving()}
                  class="inline-flex justify-center rounded-md border border-transparent bg-blue-600 py-2 px-4 text-sm font-medium text-white shadow-sm hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 disabled:opacity-50"
                >
                  {saving() ? "Saving..." : "Save Settings"}
                </button>
              </div>
            </form>
          </Show>
        </div>
      </div>
    </div>
  );
}