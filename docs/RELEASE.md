# Release Guide

Audience: bw-agent maintainers (i.e. the person cutting a release).

Status: **initial release engineering baseline (task1 output)**. Signing,
notarization and the tagged release workflow are intentionally scaffolded
but not yet fully wired — see the explicit status notes below.

---

## 1. Release goals for this phase

- Keep a **single source of truth** for the product version across
  `package.json`, `src-tauri/tauri.conf.json` and every Cargo crate.
- Ensure `pnpm run check`, `pnpm run test`, `pnpm run build` and
  `cargo build` all succeed locally before any release is cut.
- Provide a documented pre-release checklist so the first proper release
  is reproducible.
- Scaffold signing/notarization placeholders so the release workflow can
  later be promoted to a fully signed release without rediscovering
  requirements.

What is intentionally **out of scope** for this release guide:

- Auto-updater
- Channel management (stable/beta/nightly)
- Crash reporting infrastructure

## 2. Versioning source of truth

The product version must be identical across all of the following files:

| File | Field |
|------|-------|
| `package.json` | `version` |
| `src-tauri/tauri.conf.json` | `version` |
| `crates/bw-core/Cargo.toml` | `[package].version` |
| `crates/bw-agent/Cargo.toml` | `[package].version` |
| `src-tauri/Cargo.toml` | `[package].version` (crate `bw-agent-desktop`) |

Current policy (initial baseline):

- **`package.json` is the source of truth** for the human-readable product
  version (for example `0.1.0`).
- The workspace root `Cargo.toml` intentionally does **not** set a unified
  `[workspace.package].version`; each crate sets its own to keep
  flexibility between library crates and the desktop binary.
- When those per-crate versions diverge from `package.json`, we update
  them in the same commit to keep release bookkeeping simple.

If `package.json` ever carries a placeholder version (for example the
historical `0.0.0`), the audit commit that exposes that case must pick
the next actual target version and update all files in one commit
(see `Task 9` in `.sisyphus/plans/2026-04-17-release-hardening-task1.md`).

## 3. Pre-release checklist

Run these locally before cutting a release (CI must also pass — see §5):

- [ ] Working tree is clean (`git status` reports no changes).
- [ ] Version fields in all files from §2 match and are the intended
      release version.
- [ ] `pnpm install --frozen-lockfile` succeeds (lockfile is up-to-date
      with `package.json`).
- [ ] `pnpm run check` passes (`cargo fmt --check` + `cargo clippy
      --workspace --all-targets -- -D warnings`).
- [ ] `pnpm run test` passes (`cargo test --workspace` — currently
      34 tests: 28 in `bw-agent`, 6 in `bw-core`).
- [ ] `pnpm run build` produces `dist/` successfully.
- [ ] `pnpm tauri build` succeeds on every platform in the release
      matrix (see §4). If a platform build cannot run on the local
      machine, rely on the `release.yml` workflow instead and record
      the run id in the release notes.
- [ ] `.github/workflows/ci.yml` has a green run on the release commit.

## 4. Platform matrix for release artifacts

First-pass release matrix (per the task1 default decision):

| Platform | Runner | Notes |
|----------|--------|-------|
| Windows x64 | `windows-latest` | Tauri bundles NSIS installer and/or MSI |
| macOS aarch64 | `macos-latest` | Apple Silicon; see §6 for signing/notarization status |
| macOS x86_64 | `macos-latest` (cross) | Optional; enable if Intel Mac support is required |

Linux is **not** a declared release target in this phase. If Linux
support is added later, it should be scoped as a separate task and the
Tauri Linux deps already handled in `.github/actions/prepare-build` can
be reused as-is.

## 5. CI gating

`.github/workflows/ci.yml` provides the minimal quality gate:

- `check` job: `pnpm run check` (fmt + clippy) on Linux.
- `test` job: `pnpm run test` on Windows and macOS (the platforms with
  OS-specific code paths — pipes on Windows, NSWorkspace/CoreGraphics
  on macOS).
- `build` job: `pnpm run build` (frontend bundle) on Linux.
- `ci-result` job: aggregator whose status can be used as the required
  check on the `main` branch.

Release cannot proceed if `ci-result` on the release commit is red.

## 6. Signing & notarization status

### Windows code signing

- **Status:** NOT CONFIGURED.
- **Plan:** once an EV code signing certificate (or reasonable
  alternative) is procured, wire the following into the release
  workflow:
  - Secret `WINDOWS_SIGN_CERTIFICATE` (base64-encoded PFX)
  - Secret `WINDOWS_SIGN_PASSWORD`
  - Bundle step uses `signtool` or `osslsigncode` against the MSI/EXE
    produced by `tauri build`.
- **Effect today:** Windows users see an unsigned-binary SmartScreen
  warning on first run.

### macOS codesign + notarization

- **Status:** NOT CONFIGURED.
- **Plan:** once an Apple Developer Team ID is available, wire:
  - Secret `APPLE_TEAM_ID`
  - Secret `APPLE_CERTIFICATE` (base64 `.p12`)
  - Secret `APPLE_CERTIFICATE_PASSWORD`
  - Secret `APPLE_ID` and `APPLE_APP_SPECIFIC_PASSWORD` (for
    `notarytool submit` / `xcrun altool`)
  - `tauri build` picks up the identity automatically when Keychain
    is populated from the secrets above.
- **Effect today:** macOS users see an unidentified-developer dialog
  and must explicitly allow the app the first time.

### Current behaviour of `release.yml`

See `.github/workflows/release.yml`. The first version of that
workflow intentionally produces **unsigned** Tauri bundles and uploads
them as workflow artifacts only; promoting artifacts to signed
GitHub releases is explicitly a follow-up task.

## 7. Cut-a-release procedure (phase 1)

Until full signing is wired up, a release is a manual step:

1. Decide the target version (follow semver; start at `0.1.0` for the
   initial baseline).
2. Update all files from §2 in a single commit:
   ```
   chore: release v<version>
   ```
3. Verify the pre-release checklist from §3.
4. Tag the commit: `git tag v<version>`.
5. Push branch + tag: `git push origin main --tags`.
6. Trigger `release.yml` via `workflow_dispatch` against the tag (or
   wait for the on-push-tag trigger once that is enabled).
7. Download the artifacts from the workflow run, smoke-test each
   platform binary locally, and then promote to a GitHub Release when
   signing is configured.

## 8. Phase 1 scope acknowledgement

This guide is the phase-1 release baseline shipped by task1. It is
intentionally incomplete with respect to:

- Automated GitHub Releases from tags
- Signing/notarization pipelines
- Auto-updates

Those are tracked as follow-ups and should be promoted from this doc
incrementally as each piece lands.
