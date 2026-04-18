# 发布指南

面向对象：bw-agent 维护者（即负责发版的人）。

---

## 1. 快速参考 — 如何发版

```bash
# 1. 更新版本号（§3 中所有文件的版本必须一致）
# 2. 提交并打标签
git commit -am "chore: release v0.2.0"
git tag v0.2.0
git push origin main --tags
```

搞定。剩下的交给 GitHub Actions：

1. **`release.yml`** 检测到 `v*` 标签，并行构建 6 个平台目标，将产物打包为 zip，并创建一个**草稿 GitHub Release**。
2. 你在 Releases 页面审核草稿，需要的话做冒烟测试，然后点击 **Publish**。

也可以通过 **Actions → Release → Run workflow** 手动触发构建，指定任意已有标签。

---

## 2. 发布流程概览

```
 git push --tags (v*)
        │
        ▼
 ┌──────────────────────────────────────────────┐
 │            release.yml (6 个 job)            │
 │                                              │
 │  ┌─────────────┐  ┌──────────────────────┐   │
 │  │ build (×6)  │─▶│ publish              │   │
 │  │             │  │  下载全部 artifacts   │   │
 │  │ linux-x64   │  │  每个 artifact 打 zip │   │
 │  │ linux-arm64 │  │  创建草稿 Release    │   │
 │  │ macos-x64   │  └──────────────────────┘   │
 │  │ macos-arm64 │                              │
 │  │ win-x64     │                              │
 │  │ win-arm64   │                              │
 │  └─────────────┘                              │
 └──────────────────────────────────────────────┘
        │
        ▼
  GitHub 上的草稿 Release
  （自动生成的 changelog）
  → 审核 → 点击 Publish
```

---

## 3. 版本号单一来源

产品版本在以下所有文件中必须保持一致：

| 文件 | 字段 |
|------|------|
| `package.json` | `version` |
| `src-tauri/tauri.conf.json` | `version` |
| `crates/bw-core/Cargo.toml` | `[package].version` |
| `crates/bw-agent/Cargo.toml` | `[package].version` |
| `src-tauri/Cargo.toml` | `[package].version`（crate `bw-agent-desktop`） |

**`package.json` 是版本唯一来源。** 先改它，再把其他四个文件改成一样的版本号，放在同一个 commit 里。

---

## 4. 平台构建矩阵

| 产物名称 | Runner | Rust target | 输出格式 |
|---|---|---|---|
| `bw-agent-linux-x86_64` | `ubuntu-24.04` | `x86_64-unknown-linux-gnu` | `.deb`、`.rpm`、`.AppImage` |
| `bw-agent-linux-aarch64` | `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | `.deb`、`.rpm`、`.AppImage` |
| `bw-agent-macos-x86_64` | `macos-latest` | `x86_64-apple-darwin` | `.dmg`、`.app.tar.gz` |
| `bw-agent-macos-aarch64` | `macos-latest` | `aarch64-apple-darwin` | `.dmg`、`.app.tar.gz` |
| `bw-agent-windows-x86_64` | `windows-latest` | `x86_64-pc-windows-msvc` | `.msi`、`.exe`（NSIS） |
| `bw-agent-windows-arm64` | `windows-11-arm` | `aarch64-pc-windows-msvc` | `.msi`、`.exe`（NSIS） |

备注：

- **macOS**：两个架构都在 `macos-latest`（ARM runner）上构建；x86_64 通过 `--target x86_64-apple-darwin` 交叉编译。
- **Linux ARM64**：使用原生 `ubuntu-24.04-arm` runner，Tauri 的系统依赖（webkit2gtk、GTK3 等）可以直接通过 apt 安装。
- **Windows ARM64**：使用 `windows-11-arm` runner。

### 构建原理（参考 EasyTier）

构建由 `tauri-apps/tauri-action@v0` 配合 `--target <triple>` 驱动。组合 Action
`.github/actions/prepare-build` 负责准备环境：

- Node.js + pnpm（通过 `.github/actions/prepare-pnpm`）
- 带指定 target triple 的 Rust 工具链
- Linux 上的 Tauri 系统依赖（webkit2gtk-4.1、GTK3 等）

`tauri-action` 完成后，一个 shell 步骤从
`./src-tauri/target/<triple>/release/bundle/` 收集对应平台的安装包文件并上传为命名 artifact。

### `publish` job

6 个 build job 全部成功后，`publish` job 执行：

1. 通过 `actions/download-artifact@v4` 下载全部 artifact。
2. 将每个 artifact 目录打包为
   `<artifact-name>-<version>.zip`（例如
   `bw-agent-linux-x86_64-v0.2.0.zip`）。
3. 调用 `softprops/action-gh-release@v2` 创建**草稿** GitHub Release，并自动生成 release notes。

草稿**不会自动发布**——需要你审核后手动点击 Publish。

---

## 5. 发版前检查清单

打标签之前，在本地执行以下检查：

- [ ] 工作区干净（`git status` 无变更）。
- [ ] §3 中所有文件的版本号一致，且为本次目标版本。
- [ ] `pnpm install --frozen-lockfile` 成功。
- [ ] `pnpm run check` 通过（`cargo fmt --check` + `cargo clippy`）。
- [ ] `pnpm run test` 通过（`cargo test --workspace`）。
- [ ] `pnpm run build` 成功生成 `dist/`。
- [ ] `.github/workflows/ci.yml` 在发版 commit 上是绿灯
      （CI 现在在 Windows、macOS **和** Linux 上都会运行测试）。

---

## 6. CI 门禁

`.github/workflows/ci.yml` 在每次推送到 `main` 或创建 PR 时运行：

| Job | 作用 | Runner |
|-----|------|--------|
| `check` | `cargo fmt --check` + `cargo clippy` | `ubuntu-latest` |
| `test` | `cargo test --workspace` | `windows-latest`、`macos-latest`、`ubuntu-latest` |
| `build` | `pnpm run build`（前端） | `ubuntu-latest` |
| `ci-result` | 聚合器——上面任意 job 失败则整体失败 | `ubuntu-latest` |

发版 commit 上的 `ci-result` 如果是红色，不允许发版。

---

## 7. 手动触发构建

如果需要为已有标签重新构建产物（例如修复构建问题）：

1. 进入 **Actions → Release**。
2. 点击 **Run workflow**。
3. 输入标签（例如 `v0.2.0`）。
4. Workflow 会 checkout 该标签，构建全部 6 个目标，并创建（或更新）草稿 Release。

---

## 8. 签名与公证（尚未配置）

### Windows 代码签名

- **状态**：未配置。
- **所需 Secrets**：`WINDOWS_SIGN_CERTIFICATE`（base64 编码的 PFX）、`WINDOWS_SIGN_PASSWORD`。
- **当前影响**：Windows 用户首次运行时会看到 SmartScreen 警告。

### macOS 代码签名 + 公证

- **状态**：未配置。
- **所需 Secrets**：`APPLE_TEAM_ID`、`APPLE_CERTIFICATE`（base64 编码的 `.p12`）、`APPLE_CERTIFICATE_PASSWORD`、`APPLE_ID`、`APPLE_APP_SPECIFIC_PASSWORD`。
- **当前影响**：macOS 用户首次运行时会看到"未识别开发者"对话框，需要手动允许。

配置签名后，在 `build` job 中（"Collect Tauri artifacts" 步骤之前）添加签名步骤，然后删除本说明。

---

## 9. 文件索引

| 文件 | 用途 |
|------|------|
| `.github/workflows/ci.yml` | CI：lint、测试、前端构建 |
| `.github/workflows/release.yml` | 构建 6 个目标 + 创建草稿 GitHub Release |
| `.github/actions/prepare-build/action.yml` | 组合 Action：Node/pnpm + Rust + Tauri Linux 依赖 |
| `.github/actions/prepare-pnpm/action.yml` | 组合 Action：Node.js + pnpm 及 store 缓存 |
| `src-tauri/tauri.conf.json` | Tauri 打包配置（产品名、标识符、图标） |
| `docs/RELEASE.md` | 本文件 |
