import { createSignal, onMount, onCleanup, Show } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import PasswordInput from "../components/PasswordInput";
import TotpInput from "../components/TotpInput";
import {
  unlock,
  submitPassword,
  submitTwoFactor,
  unlockWithTwoFactor,
  getConfig,
  saveConfig,
  type UnlockResult,
} from "../lib/tauri";
import { setStore } from "../lib/store";

type Stage = "password" | "submitting" | "two_factor" | "submitting_2fa" | "cooldown";

function navigate(path: string) {
  window.location.hash = "#" + path;
}

export default function LoginPage() {
  const [stage, setStage] = createSignal<Stage>("password");
  const [email, setEmail] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [error, setError] = createSignal<string | undefined>();
  const [attempts, setAttempts] = createSignal(0);
  const [providers, setProviders] = createSignal<number[]>([]);
  const [serverUrl, setServerUrl] = createSignal<string | null>(null);
  const [twoFactorSource, setTwoFactorSource] = createSignal<"ui" | "ssh">("ui");

  // Setup flow state (when config is empty)
  const [isSetup, setIsSetup] = createSignal(false);
  const [setupStage, setSetupStage] = createSignal<"server" | "email" | "password">("server");
  const [serverChoice, setServerChoice] = createSignal<"official" | "self-hosted" | null>(null);
  const [customUrl, setCustomUrl] = createSignal("");
  const [setupEmail, setSetupEmail] = createSignal("");
  const [savingConfig, setSavingConfig] = createSignal(false);

  let unlistenPassword: UnlistenFn | undefined;
  let unlistenTwoFactor: UnlistenFn | undefined;

  onMount(async () => {
    // Load email and server URL from config
    try {
      const config = await getConfig();
      if (config.email) {
        setEmail(config.email);
        setIsSetup(false);
      } else {
        setIsSetup(true);
        setSetupStage("server");
      }
      if (config.base_url) setServerUrl(config.base_url);
    } catch (e) {
      console.error("Failed to load config:", e);
    }

    // Listen for password requests from SSH agent thread
    unlistenPassword = await listen<{ email: string; error: string | null }>(
      "password-requested",
      (event) => {
        if (event.payload.email) setEmail(event.payload.email);
        if (event.payload.error) setError(event.payload.error);
        setStage("password");
        setPassword("");
      }
    );

    // Listen for 2FA requests from SSH agent thread
    unlistenTwoFactor = await listen<{ providers: number[] }>(
      "two-factor-requested",
      (event) => {
        setProviders(event.payload.providers);
        setTwoFactorSource("ssh");
        setStage("two_factor");
      }
    );
  });

  onCleanup(() => {
    unlistenPassword?.();
    unlistenTwoFactor?.();
  });

  // ── Setup handlers ──────────────────────────────────────────────

  const handleServerChoice = (choice: "official" | "self-hosted") => {
    setServerChoice(choice);
    if (choice === "official") {
      setSetupStage("email");
    }
    // self-hosted: stay on server stage to show URL input
  };

  const handleConfirmServer = () => {
    if (serverChoice() === "self-hosted" && !customUrl().trim()) return;
    setSetupStage("email");
  };

  const handleSaveConfigAndContinue = async () => {
    const emailVal = setupEmail().trim();
    if (!emailVal) return;

    setSavingConfig(true);
    setError(undefined);
    try {
      const config = await getConfig();
      config.email = emailVal;
      if (serverChoice() === "self-hosted" && customUrl().trim()) {
        config.base_url = customUrl().trim();
        setServerUrl(customUrl().trim());
      }
      await saveConfig(config);
      setEmail(emailVal);
      setIsSetup(false);
      setStore("email", emailVal);
      setStore("isSetupComplete", true);
    } catch (e: any) {
      const msg = typeof e === "string" ? e : e?.message || "Failed to save config";
      setError(msg);
    } finally {
      setSavingConfig(false);
    }
  };

  const handleSetupKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter") {
      if (setupStage() === "server" && serverChoice() === "self-hosted") {
        handleConfirmServer();
      } else if (setupStage() === "email") {
        handleSaveConfigAndContinue();
      }
    }
  };

  // ── Unlock handlers ─────────────────────────────────────────────

  const handleUnlock = async () => {
    if (!password().trim()) return;

    setStage("submitting");
    setError(undefined);

    try {
      const result: UnlockResult = await unlock(password());

      if (result === "Success") {
        setStore("locked", false);
        if (email()) setStore("email", email());
        navigate("/dashboard");
        return;
      }

      // TwoFactorRequired
      if (typeof result === "object" && "TwoFactorRequired" in result) {
        setProviders(result.TwoFactorRequired.providers);
        setTwoFactorSource("ui");
        setStage("two_factor");
        return;
      }
    } catch (e: any) {
      const msg = typeof e === "string" ? e : e?.message || "Unlock failed";
      setError(msg);

      const next = attempts() + 1;
      setAttempts(next);

      if (next >= 3) {
        setStage("cooldown");
        setTimeout(() => {
          setStage("password");
          setAttempts(0);
        }, 10_000);
      } else {
        setStage("password");
      }
    }
  };

  const handleTotpSubmit = async (code: string) => {
    setStage("submitting_2fa");
    setError(undefined);

    const sshProvider = providers()[0];
    if (sshProvider === undefined && twoFactorSource() === "ssh") {
      setError("No 2FA provider available");
      setStage("two_factor");
      return;
    }

    try {
      if (twoFactorSource() === "ssh") {
        await submitTwoFactor(sshProvider!, code);
        return;
      }

      // UI path: explicitly use Authenticator (0) since TotpInput only shows for this provider.
      const result: UnlockResult = await unlockWithTwoFactor(0, code);

      if (result === "Success") {
        setStore("locked", false);
        if (email()) setStore("email", email());
        navigate("/dashboard");
        return;
      }

      if (typeof result === "object" && "TwoFactorRequired" in result) {
        setProviders(result.TwoFactorRequired.providers);
        setStage("two_factor");
        return;
      }
    } catch (e: any) {
      const msg = typeof e === "string" ? e : e?.message || "2FA verification failed";
      setError(msg);
      setStage("two_factor");
    }
  };

  const busy = () => stage() === "submitting" || stage() === "submitting_2fa" || stage() === "cooldown";

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && stage() === "password") {
      handleUnlock();
    }
  };

  return (
    <div class="min-h-screen flex flex-col items-center justify-center bg-gray-900 px-4">
      {/* Settings gear - top right */}
      <div class="fixed top-4 right-4">
        <button
          onClick={() => navigate("/settings")}
          disabled={busy()}
          class="p-2 text-zinc-500 hover:text-zinc-300 focus:outline-none focus:ring-2 focus:ring-blue-500 rounded-full transition-colors disabled:opacity-30 disabled:cursor-not-allowed disabled:hover:text-zinc-500"
          title="Settings"
        >
          <svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
          </svg>
        </button>
      </div>

      <div class="w-full max-w-sm space-y-6">
        {/* Header */}
        <div class="text-center">
          <h1 class="text-2xl font-bold text-white">Bitwarden SSH Agent</h1>
          <Show when={email()}>
            <p class="mt-2 text-sm text-zinc-400">{email()}</p>
          </Show>
          <Show when={serverUrl()}>
            <p class="mt-1 text-xs text-zinc-500">{serverUrl()}</p>
          </Show>
        </div>

        {/* ── Setup flow ─────────────────────────────────────────── */}

        {/* Setup: Server choice */}
        <Show when={isSetup() && setupStage() === "server"}>
          <div class="space-y-4">
            <p class="text-center text-sm text-zinc-400">Choose your Bitwarden server</p>
            <Show when={error()}>
              <p class="text-center text-sm text-red-500">{error()}</p>
            </Show>
            <div class="space-y-3">
              <button
                onClick={() => handleServerChoice("official")}
                class="w-full py-3 px-4 bg-zinc-800 hover:bg-zinc-700 border border-zinc-600 hover:border-blue-500 rounded-lg text-left transition-colors"
              >
                <div class="font-medium text-zinc-100">Bitwarden Cloud</div>
                <div class="text-xs text-zinc-400 mt-1">bitwarden.com</div>
              </button>
              <button
                onClick={() => handleServerChoice("self-hosted")}
                class="w-full py-3 px-4 bg-zinc-800 hover:bg-zinc-700 border border-zinc-600 hover:border-blue-500 rounded-lg text-left transition-colors"
              >
                <div class="font-medium text-zinc-100">Self-hosted Server</div>
                <div class="text-xs text-zinc-400 mt-1">Your own Bitwarden instance</div>
              </button>
            </div>
          </div>
        </Show>

        {/* Setup: Self-hosted URL input */}
        <Show when={isSetup() && setupStage() === "server" && serverChoice() === "self-hosted"}>
          <div class="space-y-3" onKeyDown={handleSetupKeyDown}>
            <input
              type="url"
              value={customUrl()}
              onInput={(e) => setCustomUrl(e.currentTarget.value)}
              placeholder="https://vault.example.com"
              class="w-full px-4 py-2 bg-zinc-900 border border-zinc-700 rounded-md text-zinc-100 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all"
              autofocus
            />
            <button
              onClick={handleConfirmServer}
              disabled={!customUrl().trim()}
              class="w-full py-2.5 px-4 bg-blue-600 hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed text-white font-medium rounded-md transition-colors"
            >
              Continue
            </button>
          </div>
        </Show>

        {/* Setup: Email input */}
        <Show when={isSetup() && setupStage() === "email"}>
          <div class="space-y-4" onKeyDown={handleSetupKeyDown}>
            <p class="text-center text-sm text-zinc-400">Enter your Bitwarden email</p>
            <Show when={error()}>
              <p class="text-center text-sm text-red-500">{error()}</p>
            </Show>
            <input
              type="email"
              value={setupEmail()}
              onInput={(e) => setSetupEmail(e.currentTarget.value)}
              placeholder="you@example.com"
              class="w-full px-4 py-2 bg-zinc-900 border border-zinc-700 rounded-md text-zinc-100 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent transition-all"
              autofocus
            />
            <button
              onClick={handleSaveConfigAndContinue}
              disabled={!setupEmail().trim() || savingConfig()}
              class="w-full py-2.5 px-4 bg-blue-600 hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed text-white font-medium rounded-md transition-colors"
            >
              {savingConfig() ? "Saving..." : "Continue"}
            </button>
            <button
              onClick={() => { setSetupStage("server"); setServerChoice(null); setError(undefined); }}
              disabled={savingConfig()}
              class="w-full py-2 text-sm text-zinc-400 hover:text-zinc-200 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            >
              ← Back to server selection
            </button>
          </div>
        </Show>

        {/* ── Normal login flow ───────────────────────────────────── */}

        {/* Password stage */}
        <Show when={!isSetup() && (stage() === "password" || stage() === "submitting")}>
          <div class="space-y-4" onKeyDown={handleKeyDown}>
            <PasswordInput
              value={password()}
              onInput={setPassword}
              error={error()}
              disabled={stage() === "submitting"}
              placeholder="Master password"
            />
            <button
              onClick={handleUnlock}
              disabled={stage() === "submitting" || !password().trim()}
              class="w-full py-2.5 px-4 bg-blue-600 hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed text-white font-medium rounded-md transition-colors focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2 focus:ring-offset-gray-900"
            >
              {stage() === "submitting" ? "Unlocking..." : "Unlock"}
            </button>
            <Show when={attempts() > 0 && attempts() < 3}>
              <p class="text-center text-xs text-zinc-500">
                Attempt {attempts()} of 3
              </p>
            </Show>
          </div>
        </Show>

        {/* 2FA stage */}
        <Show when={!isSetup() && (stage() === "two_factor" || stage() === "submitting_2fa")}>
          <div class="space-y-4">
            <p class="text-center text-sm text-zinc-300">
              Two-factor authentication required
            </p>
            <Show when={error()}>
              <p class="text-center text-sm text-red-500">{error()}</p>
            </Show>
            <Show when={providers().includes(0)}>
              <TotpInput
                onSubmit={handleTotpSubmit}
                disabled={stage() === "submitting_2fa"}
              />
            </Show>
            <button
              onClick={() => { setStage("password"); setError(undefined); }}
              disabled={stage() === "submitting_2fa"}
              class="w-full py-2 text-sm text-zinc-400 hover:text-zinc-200 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
            >
              ← Back to password
            </button>
          </div>
        </Show>

        {/* Cooldown stage */}
        <Show when={!isSetup() && stage() === "cooldown"}>
          <div class="text-center space-y-3">
            <p class="text-red-400 font-medium">Too many failed attempts</p>
            <p class="text-sm text-zinc-400">Please wait before trying again...</p>
          </div>
        </Show>
      </div>
    </div>
  );
}
