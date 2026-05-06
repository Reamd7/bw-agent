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
  createAuthRequest,
  pollAuthRequest,
  cancelAuthRequest,
  submitAuthRequestTwoFactor,
  hasRegisteredDevice,
  type UnlockResult,
} from "../lib/tauri";
import { setStore } from "../lib/store";

type Stage = "password" | "submitting" | "two_factor" | "submitting_2fa" | "cooldown" | "device_login" | "device_login_polling" | "device_login_2fa";

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
  
  const [rememberDevice, setRememberDevice] = createSignal(false);
  const [twoFactorMode, setTwoFactorMode] = createSignal<"authenticator" | "recovery">("authenticator");
  const [recoveryCode, setRecoveryCode] = createSignal("");
  const [deviceFingerprint, setDeviceFingerprint] = createSignal("");
  const [canDeviceLogin, setCanDeviceLogin] = createSignal(false);

  // Setup flow state (when config is empty)
  const [isSetup, setIsSetup] = createSignal(false);
  const [setupStage, setSetupStage] = createSignal<"server" | "email" | "password">("server");
  const [serverChoice, setServerChoice] = createSignal<"official" | "self-hosted" | null>(null);
  const [customUrl, setCustomUrl] = createSignal("");
  const [setupEmail, setSetupEmail] = createSignal("");
  const [savingConfig, setSavingConfig] = createSignal(false);

  let unlistenPassword: UnlistenFn | undefined;
  let unlistenTwoFactor: UnlistenFn | undefined;
  let pollingInterval: number | undefined;

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

      try {
        setCanDeviceLogin(await hasRegisteredDevice());
      } catch {
        // Not configured yet — ignore
      }
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
    if (pollingInterval) clearInterval(pollingInterval);
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
      const result: UnlockResult = await unlockWithTwoFactor(0, code, rememberDevice());

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

  const handleRecoverySubmit = async () => {
    if (!recoveryCode().trim()) return;
    setStage("submitting_2fa");
    setError(undefined);

    try {
      const result: UnlockResult = await unlockWithTwoFactor(8, recoveryCode(), rememberDevice());

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
      const msg = typeof e === "string" ? e : e?.message || "Recovery code verification failed";
      setError(msg);
      setStage("two_factor");
    }
  };

  const handleDeviceLogin = async () => {
    setStage("device_login");
    setError(undefined);
    try {
      const res = await createAuthRequest();
      setDeviceFingerprint(res.fingerprint);
      setStage("device_login_polling");
      
      pollingInterval = window.setInterval(async () => {
        try {
          const pollRes = await pollAuthRequest();
          if (pollRes.approved && pollRes.two_factor_required) {
            // Approved but needs 2FA — switch to 2FA input
            clearInterval(pollingInterval);
            setProviders(pollRes.two_factor_required);
            setTwoFactorSource("ui");
            setStage("device_login_2fa");
          } else if (pollRes.approved) {
            clearInterval(pollingInterval);
            setStore("locked", false);
            if (email()) setStore("email", email());
            navigate("/dashboard");
          }
        } catch (e: any) {
          clearInterval(pollingInterval);
          setError(typeof e === "string" ? e : e?.message || "Device login failed");
          setStage("password");
        }
      }, 3000);
    } catch (e: any) {
      setError(typeof e === "string" ? e : e?.message || "Failed to start device login");
      setStage("password");
    }
  };

  const handleCancelDeviceLogin = async () => {
    if (pollingInterval) clearInterval(pollingInterval);
    try {
      await cancelAuthRequest();
    } catch (e) {
      console.error("Failed to cancel auth request", e);
    }
    setStage("password");
  };

  const busy = () => stage() === "submitting" || stage() === "submitting_2fa" || stage() === "cooldown";

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && stage() === "password") {
      handleUnlock();
    }
  };

  return (
    <div class="login-bg min-h-screen flex flex-col items-center justify-center px-4">
      {/* Settings gear - top right */}
      <div class="fixed top-4 right-4">
        <button
          onClick={() => navigate("/settings")}
          disabled={busy()}
          class="btn-ghost"
          style={{ "border-radius": "var(--radius-full)", padding: "8px" }}
          title="Settings"
        >
          <svg class="h-5 w-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
          </svg>
        </button>
      </div>

      <div class="login-card">
        {/* Header */}
        <div class="text-center mb-8">
          {/* Vault icon */}
          <div class="mx-auto mb-4 flex h-14 w-14 items-center justify-center">
            <svg width="56" height="56" viewBox="0 0 100 100" fill="none">
              <rect width="100" height="100" rx="18" fill="#4f46e5" />
              <circle cx="50" cy="50" r="30" fill="#FFFFFF" />
              <rect x="44" y="14" width="12" height="13" rx="3" fill="#4f46e5" />
              <rect x="44" y="73" width="12" height="13" rx="3" fill="#4f46e5" />
              <rect x="14" y="44" width="13" height="12" rx="3" fill="#4f46e5" />
              <rect x="73" y="44" width="13" height="12" rx="3" fill="#4f46e5" />
              <circle cx="50" cy="50" r="17" fill="#4f46e5" />
              <circle cx="50" cy="46.5" r="5.5" fill="#FFFFFF" />
              <path d="M46 53 L46.8 62 H53.2 L54 53 Z" fill="#FFFFFF" />
            </svg>
          </div>
          <h1 class="text-xl font-semibold" style={`color: var(--text-primary)`}>Bitwarden SSH Agent</h1>
          <Show when={email()}>
            <p class="mt-1.5 text-sm" style={`color: var(--text-secondary)`}>{email()}</p>
          </Show>
          <Show when={serverUrl()}>
            <p class="mt-0.5 text-xs" style={`color: var(--text-tertiary)`}>{serverUrl()}</p>
          </Show>
        </div>

        {/* ── Setup flow ─────────────────────────────────────────── */}

        {/* Setup: Server choice */}
        <Show when={isSetup() && setupStage() === "server"}>
          <div class="space-y-4">
            <p class="text-center text-sm" style={`color: var(--text-secondary)`}>Choose your Bitwarden server</p>
            <Show when={error()}>
              <p class="text-center text-sm" style={`color: var(--danger)`}>{error()}</p>
            </Show>
            <div class="space-y-2.5">
              <button
                onClick={() => handleServerChoice("official")}
                class="card-flat w-full py-3 px-4 text-left transition-all hover:border-[var(--brand-200)] hover:bg-[var(--brand-50)]"
                style={{ cursor: "pointer" }}
              >
                <div class="font-medium text-sm" style={`color: var(--text-primary)`}>Bitwarden Cloud</div>
                <div class="text-xs mt-0.5" style={`color: var(--text-tertiary)`}>bitwarden.com</div>
              </button>
              <button
                onClick={() => handleServerChoice("self-hosted")}
                class="card-flat w-full py-3 px-4 text-left transition-all hover:border-[var(--brand-200)] hover:bg-[var(--brand-50)]"
                style={{ cursor: "pointer" }}
              >
                <div class="font-medium text-sm" style={`color: var(--text-primary)`}>Self-hosted Server</div>
                <div class="text-xs mt-0.5" style={`color: var(--text-tertiary)`}>Your own Bitwarden instance</div>
              </button>
            </div>
          </div>
        </Show>

        {/* Setup: Self-hosted URL input */}
        <Show when={isSetup() && setupStage() === "server" && serverChoice() === "self-hosted"}>
          <div class="space-y-3 mt-4" onKeyDown={handleSetupKeyDown}>
            <input
              type="url"
              value={customUrl()}
              onInput={(e) => setCustomUrl(e.currentTarget.value)}
              placeholder="https://vault.example.com"
              class="input"
              autofocus
            />
            <button
              onClick={handleConfirmServer}
              disabled={!customUrl().trim()}
              class="btn btn-primary w-full"
            >
              Continue
            </button>
          </div>
        </Show>

        {/* Setup: Email input */}
        <Show when={isSetup() && setupStage() === "email"}>
          <div class="space-y-4" onKeyDown={handleSetupKeyDown}>
            <p class="text-center text-sm" style={`color: var(--text-secondary)`}>Enter your Bitwarden email</p>
            <Show when={error()}>
              <p class="text-center text-sm" style={`color: var(--danger)`}>{error()}</p>
            </Show>
            <input
              type="email"
              value={setupEmail()}
              onInput={(e) => setSetupEmail(e.currentTarget.value)}
              placeholder="you@example.com"
              class="input"
              autofocus
            />
            <button
              onClick={handleSaveConfigAndContinue}
              disabled={!setupEmail().trim() || savingConfig()}
              class="btn btn-primary w-full"
            >
              {savingConfig() ? "Saving..." : "Continue"}
            </button>
            <button
              onClick={() => { setSetupStage("server"); setServerChoice(null); setError(undefined); }}
              disabled={savingConfig()}
              class="btn btn-ghost w-full text-sm"
            >
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M10 19l-7-7m0 0l7-7m-7 7h18" />
              </svg>
              Back to server selection
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
              class="btn btn-primary w-full"
            >
              {stage() === "submitting" ? (
                <>
                  <svg class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
                    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
                    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                  </svg>
                  Unlocking...
                </>
              ) : "Unlock"}
            </button>
            <button
              onClick={handleDeviceLogin}
              disabled={stage() === "submitting" || !canDeviceLogin()}
              class="btn btn-ghost w-full mt-2"
              title={canDeviceLogin() ? "Log in using an existing Bitwarden device" : "Log in with password first to register this device"}
            >
              Log in with device
            </button>
            <Show when={!canDeviceLogin()}>
              <p class="text-xs mt-1 text-center" style={`color: var(--text-tertiary)`}>
                Log in with password first to enable this option
              </p>
            </Show>
            <Show when={attempts() > 0 && attempts() < 3}>
              <p class="text-center text-xs" style={`color: var(--text-tertiary)`}>
                Attempt {attempts()} of 3
              </p>
            </Show>
          </div>
        </Show>

        {/* 2FA stage */}
        <Show when={!isSetup() && (stage() === "two_factor" || stage() === "submitting_2fa")}>
          <div class="space-y-4">
            <p class="text-center text-sm font-medium" style={`color: var(--text-primary)`}>
              Two-factor authentication required
            </p>
            <Show when={error()}>
              <p class="text-center text-sm" style={`color: var(--danger)`}>{error()}</p>
            </Show>
            
            <Show when={providers().includes(8)}>
              <div class="flex border-b border-[var(--brand-100)] mb-4">
                <button
                  class={`flex-1 py-2 text-sm font-medium border-b-2 ${twoFactorMode() === "authenticator" ? "border-[var(--brand-500)] text-[var(--brand-600)]" : "border-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)]"}`}
                  onClick={() => setTwoFactorMode("authenticator")}
                >
                  Authenticator
                </button>
                <button
                  class={`flex-1 py-2 text-sm font-medium border-b-2 ${twoFactorMode() === "recovery" ? "border-[var(--brand-500)] text-[var(--brand-600)]" : "border-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)]"}`}
                  onClick={() => setTwoFactorMode("recovery")}
                >
                  Recovery Code
                </button>
              </div>
            </Show>

            <Show when={twoFactorMode() === "authenticator" && providers().includes(0)}>
              <TotpInput
                onSubmit={handleTotpSubmit}
                disabled={stage() === "submitting_2fa"}
              />
            </Show>

            <Show when={twoFactorMode() === "recovery" && providers().includes(8)}>
              <div class="space-y-3">
                <input
                  type="text"
                  value={recoveryCode()}
                  onInput={(e) => setRecoveryCode(e.currentTarget.value)}
                  placeholder="Recovery code"
                  class="input"
                  disabled={stage() === "submitting_2fa"}
                  onKeyDown={(e) => e.key === "Enter" && handleRecoverySubmit()}
                />
                <button
                  onClick={handleRecoverySubmit}
                  disabled={stage() === "submitting_2fa" || !recoveryCode().trim()}
                  class="btn btn-primary w-full"
                >
                  {stage() === "submitting_2fa" ? "Verifying..." : "Verify"}
                </button>
              </div>
            </Show>

            <div class="flex items-center mt-4">
              <input
                type="checkbox"
                id="remember-device"
                checked={rememberDevice()}
                onChange={(e) => setRememberDevice(e.currentTarget.checked)}
                class="mr-2"
                disabled={stage() === "submitting_2fa"}
              />
              <label for="remember-device" class="text-sm" style={`color: var(--text-secondary)`}>
                Remember this device for 30 days
              </label>
            </div>

            <button
              onClick={() => { setStage("password"); setError(undefined); }}
              disabled={stage() === "submitting_2fa"}
              class="btn btn-ghost w-full text-sm mt-2"
            >
              <svg class="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M10 19l-7-7m0 0l7-7m-7 7h18" />
              </svg>
              Back to password
            </button>
          </div>
        </Show>

        {/* Device Login stage */}
        <Show when={!isSetup() && (stage() === "device_login" || stage() === "device_login_polling")}>
          <div class="space-y-6 text-center">
            <p class="text-sm font-medium" style={`color: var(--text-primary)`}>
              Log in with device
            </p>
            <p class="text-sm" style={`color: var(--text-secondary)`}>
              Approve this request on your existing Bitwarden device
            </p>
            
            <Show when={stage() === "device_login_polling"}>
              <div class="py-4">
                <p class="text-xs mb-2 uppercase tracking-wider font-semibold" style={`color: var(--text-tertiary)`}>Fingerprint Phrase</p>
                <div class="font-mono text-lg font-medium p-4 rounded-lg bg-[var(--brand-50)] text-[var(--brand-600)] border border-[var(--brand-100)]">
                  {deviceFingerprint()}
                </div>
              </div>
              <div class="flex items-center justify-center space-x-2 text-sm" style={`color: var(--text-secondary)`}>
                <svg class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
                  <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
                  <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                </svg>
                <span>Waiting for approval...</span>
              </div>
            </Show>

            <Show when={stage() === "device_login"}>
              <div class="flex items-center justify-center space-x-2 text-sm py-8" style={`color: var(--text-secondary)`}>
                <svg class="w-4 h-4 animate-spin" fill="none" viewBox="0 0 24 24">
                  <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" />
                  <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
                </svg>
                <span>Initiating request...</span>
              </div>
            </Show>

            <button
              onClick={handleCancelDeviceLogin}
              class="btn btn-ghost w-full text-sm"
            >
              Cancel
            </button>
          </div>
        </Show>

        {/* Device Login 2FA stage — approved but server requires 2FA */}
        <Show when={!isSetup() && stage() === "device_login_2fa"}>
          <div class="space-y-6">
            <p class="text-sm font-medium text-center" style={`color: var(--text-primary)`}>
              Two-factor authentication required
            </p>
            <p class="text-sm text-center" style={`color: var(--text-secondary)`}>
              Your login was approved, but two-factor authentication is still required.
            </p>

            <div class="flex border-b mb-4" style={`border-color: var(--border)`}>
              <button
                class={`flex-1 py-2 text-sm font-medium border-b-2 ${twoFactorMode() === "authenticator" ? "border-[var(--brand-500)] text-[var(--brand-600)]" : "border-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)]"}`}
                onClick={() => setTwoFactorMode("authenticator")}
              >
                Authenticator
              </button>
              <button
                class={`flex-1 py-2 text-sm font-medium border-b-2 ${twoFactorMode() === "recovery" ? "border-[var(--brand-500)] text-[var(--brand-600)]" : "border-transparent text-[var(--text-secondary)] hover:text-[var(--text-primary)]"}`}
                onClick={() => setTwoFactorMode("recovery")}
              >
                Recovery Code
              </button>
            </div>

            <Show when={twoFactorMode() === "authenticator" && providers().includes(0)}>
              <TotpInput
                onSubmit={async (code: string) => {
                  setStage("submitting_2fa");
                  try {
                    const result = await submitAuthRequestTwoFactor(
                      0, // Authenticator provider
                      code,
                      rememberDevice(),
                    );
                    if (result.success) {
                      setStore("locked", false);
                      if (email()) setStore("email", email());
                      navigate("/dashboard");
                    }
                  } catch (e: any) {
                    setError(typeof e === "string" ? e : e?.message || "2FA verification failed");
                    setStage("device_login_2fa");
                  }
                }}
                disabled={stage() === "submitting_2fa"}
              />
            </Show>

            <Show when={twoFactorMode() === "recovery" && providers().includes(8)}>
              <div class="space-y-3">
                <input
                  type="text"
                  value={recoveryCode()}
                  onInput={(e) => setRecoveryCode(e.currentTarget.value)}
                  placeholder="Recovery code"
                  class="input"
                  disabled={stage() === "submitting_2fa"}
                />
                <button
                  onClick={async () => {
                    setStage("submitting_2fa");
                    try {
                      const result = await submitAuthRequestTwoFactor(
                        8, // Recovery code provider
                        recoveryCode(),
                        false,
                      );
                      if (result.success) {
                        setStore("locked", false);
                        if (email()) setStore("email", email());
                        navigate("/dashboard");
                      }
                    } catch (e: any) {
                      setError(typeof e === "string" ? e : e?.message || "Recovery code failed");
                      setStage("device_login_2fa");
                    }
                  }}
                  class="btn btn-primary w-full"
                  disabled={!recoveryCode() || stage() === "submitting_2fa"}
                >
                  Verify with recovery code
                </button>
              </div>
            </Show>

            <label class="flex items-center gap-2 text-sm cursor-pointer" style={`color: var(--text-secondary)`}>
              <input
                type="checkbox"
                checked={rememberDevice()}
                onChange={(e) => setRememberDevice(e.currentTarget.checked)}
                class="checkbox checkbox-sm"
              />
              Remember this device for 30 days
            </label>

            <button
              onClick={handleCancelDeviceLogin}
              class="btn btn-ghost w-full text-sm"
            >
              Cancel
            </button>
          </div>
        </Show>

        {/* Cooldown stage */}
        <Show when={!isSetup() && stage() === "cooldown"}>
          <div class="text-center space-y-2">
            <div class="mx-auto mb-3 flex h-10 w-10 items-center justify-center rounded-full" style={`background: var(--danger-bg)`}>
              <svg class="h-5 w-5" style={`color: var(--danger)`} fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
                <path stroke-linecap="round" stroke-linejoin="round" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z" />
              </svg>
            </div>
            <p class="font-medium text-sm" style={`color: var(--danger)`}>Too many failed attempts</p>
            <p class="text-sm" style={`color: var(--text-tertiary)`}>Please wait before trying again...</p>
          </div>
        </Show>
      </div>
    </div>
  );
}
