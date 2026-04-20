# Deferred Email Setup — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow the Tauri app to start without email configured, guiding users through setup inline on the Login page.

**Architecture:** Remove the startup-time `validate()` call from the Tauri setup, let SSH agent always start. Add an `is_empty()` method to Config. Modify `LoginPage.tsx` to show a step-by-step setup flow (server choice → email → password) when config is empty.

**Tech Stack:** Rust (Tauri backend), SolidJS + TypeScript (frontend)

---

## Chunk 1: Backend — Allow startup without email

### Task 1: Add `is_empty()` to Config and update tests

**Files:**
- Modify: `crates/bw-agent/src/config.rs`

- [ ] **Step 1: Write failing tests for `is_empty()`**

Add to `crates/bw-agent/src/config.rs` inside `mod tests`:

```rust
#[test]
fn test_is_empty_when_no_email_no_base_url() {
    let config = Config::default();
    assert!(config.is_empty());
}

#[test]
fn test_is_empty_false_when_email_set() {
    let config = Config {
        email: Some("user@example.com".to_string()),
        ..Config::default()
    };
    assert!(!config.is_empty());
}

#[test]
fn test_is_empty_false_when_base_url_set() {
    let config = Config {
        base_url: Some("https://vault.example.com".to_string()),
        ..Config::default()
    };
    assert!(!config.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd .worktrees/deferred-email-setup && cargo test --package bw-agent config::tests::test_is_empty -- --nocapture`
Expected: compile error — `is_empty` method does not exist

- [ ] **Step 3: Implement `is_empty()`**

Add to the `impl Config` block in `crates/bw-agent/src/config.rs`:

```rust
/// Returns true when neither email nor base_url is configured.
/// Used by the Tauri frontend to decide whether to show the setup flow.
pub fn is_empty(&self) -> bool {
    self.email.is_none() && self.base_url.is_none()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd .worktrees/deferred-email-setup && cargo test --package bw-agent config::tests::test_is_empty -- --nocapture`
Expected: 3 tests PASS

- [ ] **Step 5: Run all existing tests to verify no regression**

Run: `cd .worktrees/deferred-email-setup && cargo test --workspace`
Expected: all tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/bw-agent/src/config.rs
git commit -m "feat(config): add is_empty() method for deferred setup detection"
```

---

### Task 2: Remove `validate()` from Tauri setup

**Files:**
- Modify: `src-tauri/src/main.rs`

- [ ] **Step 1: Remove the validate call and handle missing email**

In `src-tauri/src/main.rs`, inside the `.setup(|app| { ... })` closure, replace:

```rust
            let mut config = bw_agent::config::Config::load();
            config.apply_env_overrides();
            config.validate().map_err(to_tauri_error)?;
```

with:

```rust
            let mut config = bw_agent::config::Config::load();
            config.apply_env_overrides();
```

And replace:

```rust
            let mut initial_state = bw_agent::state::State::new(config.lock_mode.cache_ttl());
            initial_state.email = config.email.clone();
```

with:

```rust
            let mut initial_state = bw_agent::state::State::new(config.lock_mode.cache_ttl());
            initial_state.email = config.email.clone();
            if config.email.is_none() {
                log::info!("Email not configured — waiting for setup via UI");
            }
```

- [ ] **Step 2: Build to verify compilation**

Run: `cd .worktrees/deferred-email-setup && cargo build --package bw-agent-desktop`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/main.rs
git commit -m "feat(tauri): allow startup without email configured"
```

---

### Task 3: Make `start_agent_with_shared_state` tolerate missing email

**Files:**
- Modify: `crates/bw-agent/src/lib.rs`

- [ ] **Step 1: Remove the email bail in `start_agent_with_shared_state`**

In `crates/bw-agent/src/lib.rs`, in `start_agent_with_shared_state`, replace:

```rust
    let email = config
        .email
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Email not configured"))?;
```

with:

```rust
    let email = config.email.clone().unwrap_or_default();
```

And replace:

```rust
    {
        let mut state = state.lock().await;
        state.email = Some(email.clone());
    }

    log::info!("Email: {email}");
```

with:

```rust
    {
        let mut state = state.lock().await;
        state.email = if email.is_empty() { None } else { Some(email.clone()) };
    }

    if email.is_empty() {
        log::info!("Email: not configured — SSH agent running, vault operations will wait for setup");
    } else {
        log::info!("Email: {email}");
    }
```

Keep the rest of the function unchanged (api_url, identity_url, proxy logging, etc. all still work because they derive from `config.base_url` which has defaults).

- [ ] **Step 2: Build to verify compilation**

Run: `cd .worktrees/deferred-email-setup && cargo build --workspace`
Expected: compiles successfully

- [ ] **Step 3: Run all tests**

Run: `cd .worktrees/deferred-email-setup && cargo test --workspace`
Expected: all tests PASS

- [ ] **Step 4: Commit**

```bash
git add crates/bw-agent/src/lib.rs
git commit -m "feat(agent): tolerate missing email in shared state startup"
```

---

## Chunk 2: Frontend — Setup flow in LoginPage

### Task 4: Add `isSetupComplete` flag to store

**Files:**
- Modify: `src/lib/store.ts`

- [ ] **Step 1: Add `isSetupComplete` to the store**

In `src/lib/store.ts`, update the `AppStore` interface:

```typescript
interface AppStore {
  locked: boolean;
  pendingApprovals: ApprovalRequest[];
  email: string;
  isSetupComplete: boolean;
}
```

And update the initial store value:

```typescript
export const [store, setStore] = createStore<AppStore>({
  locked: true,
  pendingApprovals: [],
  email: "",
  isSetupComplete: true,
});
```

(`true` by default so existing flow isn't broken; LoginPage will set it to `false` when it detects empty config.)

- [ ] **Step 2: Commit**

```bash
git add src/lib/store.ts
git commit -m "feat(store): add isSetupComplete flag for setup flow"
```

---

### Task 5: Add setup UI to LoginPage

**Files:**
- Modify: `src/pages/LoginPage.tsx`

This is the main frontend change. When config has no email, the page shows a multi-step setup flow that progressively reveals fields on the same page.

- [ ] **Step 1: Add setup stage state and logic**

At the top of `LoginPage()`, add setup-related signals after existing signals:

```typescript
  // Setup flow state (when config is empty)
  const [isSetup, setIsSetup] = createSignal(false);
  const [setupStage, setSetupStage] = createSignal<"server" | "email" | "password">("server");
  const [serverChoice, setServerChoice] = createSignal<"official" | "self-hosted" | null>(null);
  const [customUrl, setCustomUrl] = createSignal("");
  const [setupEmail, setSetupEmail] = createSignal("");
  const [savingConfig, setSavingConfig] = createSignal(false);
```

- [ ] **Step 2: Update onMount to detect empty config**

Replace the existing `onMount` `try/catch` block inside `LoginPage()`:

```typescript
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
```

The key change: if `config.email` is falsy, set `isSetup(true)` and `setupStage("server")`.

- [ ] **Step 3: Add setup handler functions**

After `handleUnlock` and before `handleTotpSubmit`, add:

```typescript
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
```

- [ ] **Step 4: Add setup UI sections to the JSX**

Inside the `<div class="w-full max-w-sm space-y-6">` container, after the header block (which shows "Bitwarden SSH Agent" title) and before the existing password/2FA/cooldown `<Show>` blocks, add setup UI blocks.

**After the header section and before `/* Password stage */`, add:**

```tsx
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
```

- [ ] **Step 5: Guard existing password/2FA stages behind `!isSetup()`**

Wrap the existing password, 2FA, and cooldown `<Show>` blocks with an additional `!isSetup()` condition. Change:

```tsx
        <Show when={stage() === "password" || stage() === "submitting"}>
```

to:

```tsx
        <Show when={!isSetup() && (stage() === "password" || stage() === "submitting")}>
```

And:

```tsx
        <Show when={stage() === "two_factor" || stage() === "submitting_2fa"}>
```

to:

```tsx
        <Show when={!isSetup() && (stage() === "two_factor" || stage() === "submitting_2fa")}>
```

And:

```tsx
        <Show when={stage() === "cooldown"}>
```

to:

```tsx
        <Show when={!isSetup() && stage() === "cooldown"}>
```

- [ ] **Step 6: Build frontend to verify**

Run: `cd .worktrees/deferred-email-setup && npx rsbuild build 2>&1 | tail -5` (or the project's build command)
Expected: builds successfully, no type errors

- [ ] **Step 7: Full workspace build + test**

Run: `cd .worktrees/deferred-email-setup && cargo test --workspace`
Expected: all tests PASS

- [ ] **Step 8: Commit**

```bash
git add src/pages/LoginPage.tsx src/lib/store.ts
git commit -m "feat(ui): add inline setup flow to LoginPage for first-time users"
```

---

### Task 6: Final verification

- [ ] **Step 1: Run full test suite**

Run: `cd .worktrees/deferred-email-setup && cargo test --workspace`
Expected: all tests PASS

- [ ] **Step 2: Run clippy**

Run: `cd .worktrees/deferred-email-setup && cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings

- [ ] **Step 3: Verify git log is clean**

Run: `cd .worktrees/deferred-email-setup && git log --oneline main..HEAD`
Expected: 5 commits in logical order:
1. `feat(config): add is_empty() method`
2. `feat(tauri): allow startup without email configured`
3. `feat(agent): tolerate missing email in shared state startup`
4. `feat(store): add isSetupComplete flag`
5. `feat(ui): add inline setup flow to LoginPage`
