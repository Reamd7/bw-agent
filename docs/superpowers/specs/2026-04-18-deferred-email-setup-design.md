# 延迟配置 — 首次启动引导

## 问题

App 启动时在 `main.rs` 的 Tauri setup 中调用 `config.validate()`，如果 email 为空则 panic 退出。用户无法通过 UI 完成首次配置，必须先手动编辑配置文件或设置环境变量。

## 目标

- App 始终能启动，SSH socket 始终监听
- 首次启动时在 Login 页面内联引导用户完成配置
- 已配置用户不受影响

## 方案

### Rust 后端

#### `config.rs`

- 新增 `is_empty(&self) -> bool`：email 和 base_url 都为 None 时返回 true
- `validate()` 保留给 standalone binary，Tauri app 不再调用

#### `main.rs` setup

- 去掉 `config.validate().map_err(to_tauri_error)?`
- email 为空时 `initial_state.email = None`，agent 照常启动

#### `lib.rs` `start_agent_with_shared_state`

- 去掉 email 为空时的 `bail!`，email 为 None 时正常继续，日志提示等待配置

### 前端 `LoginPage.tsx`

当 config 为空（无 email、无 base_url）时，同一页面逐步展开引导 UI：

1. **服务器选择** — 两个按钮卡片：「Bitwarden 官方」/「自托管服务器」
2. 选自托管 → 展开 URL 输入框
3. 选完后 → 展开 email 输入框
4. 填完 email 点继续 → 调 `save_config` 保存
5. 保存成功后 → 展开 master password 输入框 → 正常 unlock 流程

当 config 已有 email 时，行为和现在完全一样。

### 不改的部分

- SSH agent 监听始终启动
- `unlock` command 已有 email 校验
- Settings 页面不变
