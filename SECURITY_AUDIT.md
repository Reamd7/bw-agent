# 安全加固与可观测性审计报告
## bw-agent SSH Agent 项目

**日期：** 2026-04-17  
**范围：** 集成 Bitwarden 保险库的 Rust SSH agent  
**评估方式：** 基于证据的安全功能盘点

---

## 1. 审计日志 ✅ 已实现

### 访问日志（基于 SQLite）
**文件：** `crates/bw-agent/src/access_log.rs`

**证据：**
- **结构化日志数据库**，包含如下 schema：
  - `id`（自增）
  - `timestamp`（通过 `datetime('now')` 自动生成）
  - `key_fingerprint`（SHA256 格式）
  - `key_name`（解密后的条目名称）
  - `client_exe`（完整可执行文件路径）
  - `client_pid`（进程 ID）
  - `approved`（布尔值：true/false）
  - `process_chain`（父进程链的 JSON 数组）

- **记录方式：** `AccessLog::record()` 在每次 SSH 签名操作时都会被调用
  - 捕获审批决策（批准/拒绝）
  - 存储完整进程链，便于取证
  - 时间戳由 SQLite 自动生成

- **查询接口：** `AccessLog::query(limit)` 按时间倒序返回最新记录
  - 可通过 Tauri IPC 暴露给 UI 展示
  - 支持通过 `limit` 参数分页

- **存储位置：** 平台相关数据目录
  - Windows：`%LOCALAPPDATA%/bw-agent/access_log.db`
  - Unix：`$XDG_DATA_HOME/bw-agent/access_log.db` 或 `~/.local/share/bw-agent/access_log.db`

**集成点：**
- `ssh_agent.rs`：记录每个签名请求的指纹、客户端信息和审批状态
- `process.rs`：捕获完整进程链（父 → 子），提供上下文

---

## 2. 结构化日志 ✅ 已实现

### 日志框架
**文件：** 多处（`env_logger` + `log` crate）

**证据：**
- **日志初始化：** 在独立模式和 Tauri 模式中都调用了 `env_logger::init()`
  - 遵循 `RUST_LOG` 环境变量
  - 支持可配置日志级别（debug、info、warn、error）

- **结构化日志点：**
  - **配置加载：** `log::info!("Loaded config from {}")`，包含文件路径
  - **API 连通性：** `log::info!("API URL: {api_url}")`、`log::info!("Identity URL: {identity_url}")`
  - **代理配置：** `log::info!("Proxy: {proxy}")`
  - **管道/套接字绑定：** `log::info!("Listening on named pipe: {pipe_name}")`、`log::info!("Listening on Unix socket: {socket_path}")`
  - **TTL 过期：** `log::info!("Vault TTL expired — locking and notifying frontend")`
  - **系统事件：** `log::info!("Vault locked due to system event: {reason}")`
  - **进程链解析：** `log::debug!("resolve_process_chain: depth={depth} current_pid={current_pid} -> parent_pid={parent_pid}")`
  - **管道 SDDL：** `log::debug!("Pipe SDDL: {sddl}")`（Windows 安全描述符）
  - **客户端 PID 提取：** `log::debug!("New session from client PID: {pid}")`
  - **锁定模式更新：** `log::info!("Lock mode updated to {:?} (cache_ttl={:?})")`

- **错误日志：**
  - 失败操作会携带上下文记录
  - 对回退行为给出警告（例如：`"StubUiCallback: no UI available"`）
  - 通过 `anyhow::Result<T>` 传播带上下文的错误

- **调试日志：**
  - 带深度跟踪的进程链遍历
  - 内存锁定失败：`eprintln!("failed to lock memory region: {e}")`
  - Windows 上的 token/handle 操作

---

## 3. 崩溃/调试导出 ⚠️ 部分实现/缺失

### 当前状态
**证据：**
- **未发现显式的崩溃转储机制**
- **未配置调试符号导出**
- **没有用于结构化崩溃报告的 panic hook**
- **错误处理：** 使用 `anyhow::Result<T>` 携带上下文传播错误
- **Tauri 集成：** 错误会被记录，但不会导出到外部服务

### 已识别缺口
1. **没有将崩溃转储导出** 到文件或远程服务
2. **没有结构化 panic 处理器** 来捕获堆栈跟踪
3. **构建流程中没有文档说明调试符号生成**
4. **Windows/Unix 上没有 minidump 或 core dump 处理机制**
5. **没有接入遥测/可观测性后端**

### 建议
- 实现带结构化日志的 panic hook
- 将崩溃转储导出到安全位置
- 考虑接入错误跟踪服务（如 Sentry 等）

---

## 4. 秘密清理与零化 ✅ 已实现

### 内存零化
**文件：** `crates/bw-core/src/locked.rs`

**证据：**
- **自定义 `Vec` 类型**，在 drop 时自动零化：
  ```rust
  impl Drop for Vec {
      fn drop(&mut self) {
          self.zero();
          self.data.as_mut().zeroize();
      }
  }
  ```

- **内存锁定（平台相关）：**
  - **Unix：** 使用 `region::lock()` 防止页面被换出到磁盘
  - **Windows：** 跳过（注释说明 `VirtualUnlock` 在 drop 时会 panic）
  - 失败时平滑降级并给出警告：`eprintln!("failed to lock memory region: {e}")`

- **敏感数据包装类型：**
  - `Password` - 使用锁定的 Vec 包装主密码
  - `Keys` - 使用锁定的 Vec 包装加密/MAC 密钥（64 字节）
  - `PasswordHash` - 使用锁定的 Vec 包装派生哈希
  - `PrivateKey` - 使用锁定的 Vec 包装 SSH 私钥

- **零化库：** 依赖中包含 `zeroize = "1.8.2"`
  - 用于 `locked.rs` 和 `cipherstring.rs`
  - 显式清零解密后的字节：`bytes.zeroize()`

- **状态清理：**
  - `State::clear()` 方法会在锁定时清除缓存密钥
  - 在 TTL 过期、系统事件和用户锁定命令时调用
  - 清理后显式丢弃状态：`drop(state)`

- **密码处理：**
  - 用户输入后会立即把密码转换为锁定的 Vec
  - 状态中从不以普通 `String` 保存
  - 认证尝试后会清除

### Cipherstring 零化
**文件：** `crates/bw-core/src/cipherstring.rs`
- 解密后的明文在使用后会被显式清零
- 防止敏感数据在内存中泄漏

---

## 5. 保留策略 ✅ 已实现

### 基于 TTL 的缓存过期
**文件：** `config.rs`、`state.rs`、`system_events.rs`

**证据：**
- **可配置锁定模式：**
  ```rust
  pub enum LockMode {
      Timeout { seconds: u64 },      // 基于 TTL（默认：900 秒）
      SystemIdle { seconds: u64 },   // 空闲检测
      OnSleep,                        // 系统睡眠事件
      OnLock,                         // 屏幕锁定事件
      OnRestart,                      // 系统重启
      Never,                          // 永不自动锁定
  }
  ```

- **TTL 执行：**
  - `State::is_unlocked()` 检查：`cached_at.elapsed() < ttl`
  - `State::is_expired()` 检测 TTL 是否超限
  - Tauri 应用中每 5 秒执行一次周期检查
  - 过期后自动锁定：`state.clear()` + 事件发送

- **默认保留时间：** 900 秒（15 分钟）
  - 可通过 `BW_CACHE_TTL` 环境变量配置
  - 持久化到配置文件：`lock_mode: { type: "timeout", seconds: 900 }`

- **按使用刷新：** `State::touch()` 会在每次 SSH 操作时刷新 TTL
  - 避免活跃使用期间被过早锁定

- **访问日志保留：** 没有显式清理策略
  - SQLite 数据库会无限增长
  - **缺口：** 未记录任何保留策略或清理机制

### 基于系统事件的锁定
**文件：** `system_events.rs`

**证据：**
- **Windows 事件：**
  - `WM_WTSSESSION_CHANGE` - 会话锁定/解锁
  - `WM_POWERBROADCAST` - 睡眠/唤醒
  - `WM_QUERYENDSESSION` - 关机/重启

- **macOS 事件：**
  - NSWorkspace 睡眠/唤醒通知
  - `com.apple.screenIsLocked` 分布式通知

- **空闲检测：**
  - 轮询线程检查系统空闲时间
  - 阈值可配置（默认：禁用）
  - 空闲超时后触发锁定

---

## 6. 威胁模型文档 ⚠️ 缺失

### 当前状态
**证据：**
- **仓库中未发现威胁模型文档**
- **`/docs` 中没有安全设计文档**
- **没有 `SECURITY.md` 或安全策略文件**
- **没有记录攻击面分析**

### 隐式威胁模型（根据代码推断）
根据实现，项目看起来默认考虑了以下内容：

**已缓解的威胁：**
1. **未授权 SSH 密钥访问** - 审批队列 + 访问日志
2. **内存泄露** - 零化 + 内存锁定
3. **保险库缓存污染** - 基于 TTL 的过期 + 系统事件锁定
4. **未授权管道/套接字访问** - Windows SDDL 安全描述符
5. **进程伪装** - 进程链校验 + 父 PID 验证

**未被显式处理的威胁：**
1. **父进程被入侵** - 虽然记录了进程链，但并未验证
2. **保险库服务器被攻陷** - 未记录证书固定（certificate pinning）
3. **中间人攻击** - 默认依赖 HTTPS，但未加固
4. **审批 UI 被伪造** - 未记录反伪造措施
5. **拒绝服务** - 无速率限制或资源限制

### 建议
- 创建 `THREAT_MODEL.md`，记录：
  - 威胁参与者及其能力
  - 攻击场景与缓解措施
  - 假设条件与限制
  - 不在范围内的威胁

---

## 7. 管道/套接字权限加固 ✅ 已实现

### Windows 命名管道安全
**文件：** `crates/bw-agent/src/pipe.rs`

**证据：**
- **自定义 SDDL（安全描述符定义语言）：**
  ```
  D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x12019b;;;AU)(A;;GA;;;{SID})
  ```

- **ACE 说明：**
  - `SY`（SYSTEM）：完全控制（`GA`）- 便于服务互操作
  - `BA`（内置管理员组）：完全控制（`GA`）
  - `AU`（已认证用户）：`0x12019b` - 可读/写，但**不能** `CREATE_PIPE_INSTANCE`
    - 防止恶意创建伪造服务器实例
  - 当前用户 SID：完全控制（`GA`）- 允许创建实例

- **实现方式：**
  - `SecureNamedPipeListener::bind()` 使用 SDDL 创建首个管道实例
  - `create_pipe()` 通过 `ServerOptions::create_with_security_attributes_raw()` 应用安全描述符
  - `accept()` 循环会为每个客户端创建新实例（不是 `first_instance` 标记）
  - 通过 `GetNamedPipeClientProcessId()` 提取客户端 PID，用于审计日志

- **参考来源：** 参考官方 Windows OpenSSH ssh-agent（`PowerShell/openssh-portable`）

### Unix 套接字安全
**文件：** `crates/bw-agent/src/lib.rs`

**证据：**
- **套接字路径：** 从 `SSH_AUTH_SOCK` 或 `$XDG_RUNTIME_DIR/bw-agent.sock` 派生
- **清理：** 启动时执行 `std::fs::remove_file(&socket_path)`
- **权限：** 依赖 Unix 套接字默认权限（受 umask 影响）
- **缺口：** 未记录显式 `chmod 600` 或权限加固逻辑

### 建议
- 记录 Unix 套接字的权限预期
- 考虑在套接字创建后显式执行 `chmod 600`
- 启动时增加权限校验

---

## 8. 其他安全特性

### 进程链校验
**文件：** `crates/bw-agent/src/process.rs`

**证据：**
- **进程树遍历：** 最多向上解析 10 层父进程
- **硬停止列表：** 在系统进程处停止（`explorer.exe`、`sshd`、`systemd` 等）
- **透明列表：** 会包含 shell 包装进程（`cmd.exe`、`bash` 等）但继续向上遍历
- **回退：** PID 解析失败时返回 `"unknown"`
- **审计日志：** 完整链路写入访问日志，便于取证

### 审批队列
**文件：** `crates/bw-agent/src/approval.rs`

**证据：**
- **按请求独立通道：** 使用 oneshot channel 异步返回审批结果
- **请求元数据：** 包含密钥指纹、客户端 exe、PID、进程链、时间戳
- **超时处理：** 审批请求可能会超时（receiver 被丢弃）
- **审计轨迹：** 审批结果（批准/拒绝）会被记录

### 认证与会话管理
**文件：** `crates/bw-agent/src/auth.rs`

**证据：**
- **3 次尝试限制：** 密码重试有次数上限，并带错误反馈
- **2FA 支持：** 支持多种双因素认证提供方类型
- **会话令牌：** 管理 access/refresh token
- **KDF 参数：** 会被保存，用于无需再次请求服务器的重新解锁
- **受保护密钥：** 加密后的密钥材料会被保存，用于离线解锁

---

## 9. 依赖安全

### 关键安全依赖
**文件：** `crates/bw-core/Cargo.toml`

**证据：**
- `zeroize = "1.8.2"` - 内存零化
- `region = "3.0.2"` - 内存锁定
- `argon2 = "0.5.3"` - KDF（Argon2id）
- `pbkdf2 = "0.12.2"` - KDF（PBKDF2）
- `aes = "0.9.0"` - AES 加密
- `hmac = "0.13.0"` - HMAC 认证
- `rsa = "0.9.10"` - RSA 密钥处理
- `sha2 = "0.11.0"` - SHA-256/512 哈希

**观察：**
- 所有密码学库都处于良好维护状态
- 未发现明显过时或无人维护的依赖
- 已显式引入零化和内存锁定能力

---

## 10. 缺口与建议

### 关键缺口
1. **没有崩溃转储导出** - 应实现结构化 panic 处理器
2. **没有威胁模型文档** - 创建 `THREAT_MODEL.md`
3. **没有访问日志保留策略** - 定义清理/归档方案
4. **没有 Unix 套接字权限加固** - 增加显式 `chmod 600`

### 高优先级
5. **没有证书固定** - 考虑固定 Bitwarden API 证书
6. **没有速率限制** - 为审批请求增加速率限制
7. **没有审计日志加密** - 考虑加密 `access_log.db`
8. **没有日志轮转** - 实现 SQLite 日志轮转/归档

### 中优先级
9. **没有安全头文档** - 记录 HTTP 安全假设
10. **没有供应链安全措施** - 增加 SBOM 或依赖审计流程
11. **没有事件响应预案** - 记录泄露/入侵响应流程
12. **没有安全测试** - 增加 fuzzing 或面向安全的测试

### 低优先级
13. **没有 security.txt** - 可考虑发布安全策略
14. **没有 CVE 跟踪** - 配置自动依赖扫描
15. **没有安全审计追踪后端** - 可考虑不可变审计日志后端

---

## 11. 合规说明

### 相关标准
- **NIST SP 800-53：** AC-2（账户管理）、AU-2（审计事件）、SC-7（边界保护）
- **CIS Controls：** v8 3.3（应对未授权软件）、8.1（审计日志）
- **OWASP：** A01:2021（访问控制失效）、A09:2021（日志与监控）

### 当前对齐情况
- ✅ 已实现审计日志（AU-2）
- ✅ 已实现内存保护（SC-7）
- ✅ 通过审批队列实现访问控制（AC-2）
- ⚠️ 日志与监控仅部分实现（缺少崩溃转储）
- ❌ 没有正式威胁模型（合规通常要求）

---

## 12. 汇总表

| 功能 | 状态 | 证据 | 缺口 |
|---------|--------|----------|------|
| **审计日志** | ✅ 已实现 | SQLite 访问日志，含完整上下文 | 无保留策略 |
| **结构化日志** | ✅ 已实现 | `env_logger` + `log` crate | 无日志轮转 |
| **崩溃导出** | ⚠️ 部分实现 | 仅有错误处理 | 无转储导出 |
| **秘密清理** | ✅ 已实现 | 零化 + 内存锁定 | Windows 交换保护禁用 |
| **保留策略** | ✅ 已实现 | TTL + 系统事件 | 无日志清理 |
| **威胁模型** | ❌ 缺失 | 仅可从代码推断 | 无文档 |
| **管道安全** | ✅ 已实现 | Windows SDDL 加固 | Unix 权限隐式 |
| **套接字安全** | ⚠️ 部分实现 | 仅路径派生 | 无 `chmod 600` |
| **进程校验** | ✅ 已实现 | 进程链遍历 + 硬停止列表 | 无验证 |
| **审批队列** | ✅ 已实现 | 按请求独立通道 | 无超时处理 |

---

## 13. 结论

bw-agent 项目展现了**扎实的基础安全实践**，包括：
- ✅ 完整的审计日志
- ✅ 正确的秘密零化
- ✅ 内存保护机制
- ✅ Windows 管道安全加固
- ✅ 基于 TTL 的缓存过期

**但仍存在关键缺口：**
- ❌ 没有崩溃转储导出机制
- ❌ 没有威胁模型文档
- ❌ 没有访问日志保留策略
- ⚠️ Unix 套接字权限未显式加固

**建议：** 在生产部署前优先补齐关键缺口，尤其是威胁模型文档和用于事件响应的崩溃转储导出能力。

---

**报告生成时间：** 2026-04-17  
**审计方：** 安全加固与可观测性检查  
**置信等级：** 高（基于证据与代码审查）

（文件结束 - 共 419 行）
