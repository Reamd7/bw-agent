import { createSignal, onMount, onCleanup, Show } from "solid-js";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import PasswordInput from "../components/PasswordInput";
import TotpInput from "../components/TotpInput";
import {
  unlock,
  submitPassword,
  submitTwoFactor,
  getConfig,
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

  let unlistenPassword: UnlistenFn | undefined;
  let unlistenTwoFactor: UnlistenFn | undefined;

  onMount(async () => {
    // Load email from config
    try {
      const config = await getConfig();
      if (config.email) setEmail(config.email);
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
        setStage("two_factor");
      }
    );
  });

  onCleanup(() => {
    unlistenPassword?.();
    unlistenTwoFactor?.();
  });

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

    try {
      await submitTwoFactor(0, code);
      // After 2FA, try unlock again with the same password
      const result = await unlock(password());
      if (result === "Success") {
        setStore("locked", false);
        if (email()) setStore("email", email());
        navigate("/dashboard");
      } else {
        setError("Unlock failed after 2FA");
        setStage("two_factor");
      }
    } catch (e: any) {
      const msg = typeof e === "string" ? e : e?.message || "2FA failed";
      setError(msg);
      setStage("two_factor");
    }
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Enter" && stage() === "password") {
      handleUnlock();
    }
  };

  return (
    <div class="min-h-screen flex flex-col items-center justify-center bg-gray-900 px-4">
      <div class="w-full max-w-sm space-y-6">
        {/* Header */}
        <div class="text-center">
          <h1 class="text-2xl font-bold text-white">Bitwarden SSH Agent</h1>
          <Show when={email()}>
            <p class="mt-2 text-sm text-zinc-400">{email()}</p>
          </Show>
        </div>

        {/* Password stage */}
        <Show when={stage() === "password" || stage() === "submitting"}>
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
        <Show when={stage() === "two_factor" || stage() === "submitting_2fa"}>
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
              class="w-full py-2 text-sm text-zinc-400 hover:text-zinc-200 transition-colors"
            >
              ← Back to password
            </button>
          </div>
        </Show>

        {/* Cooldown stage */}
        <Show when={stage() === "cooldown"}>
          <div class="text-center space-y-3">
            <p class="text-red-400 font-medium">Too many failed attempts</p>
            <p class="text-sm text-zinc-400">Please wait before trying again...</p>
          </div>
        </Show>
      </div>
    </div>
  );
}
