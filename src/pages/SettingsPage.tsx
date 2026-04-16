import { createSignal, onMount, Show } from "solid-js";
import { getConfig, saveConfig, lockVault, type Config } from "../lib/tauri";
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
    lock_timeout: 3600,
    proxy: "",
  });
  const [loading, setLoading] = createSignal(true);
  const [saving, setSaving] = createSignal(false);
  const [toast, setToast] = createSignal<{ message: string; type: "success" | "error" } | null>(null);
  let originalConfig: Config | null = null;

  onMount(async () => {
    try {
      const currentConfig = await getConfig();
      setConfig(currentConfig);
      originalConfig = { ...currentConfig };
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
                    <label for="lock_timeout" class="block text-sm font-medium text-gray-700">
                      Lock Timeout (seconds)
                    </label>
                    <div class="mt-1">
                      <input
                        type="number"
                        id="lock_timeout"
                        min="0"
                        value={config().lock_timeout}
                        onInput={(e) => updateField("lock_timeout", parseInt(e.currentTarget.value) || 0)}
                        class="block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 sm:text-sm px-3 py-2 border"
                      />
                    </div>
                    <p class="mt-2 text-xs text-gray-500">
                      Set to 0 to disable auto-lock.
                    </p>
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