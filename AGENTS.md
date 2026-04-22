# Project Knowledge Base

## Tauri Build Tips

### 替换图标后不需要 `cargo clean`

`tauri_build::build()` 的 `cargo:rerun-if-changed` **不监听 `icons/` 目录**，只监听 `tauri.conf.json`、`capabilities/`、sidecar binary。所以替换图标文件后 Cargo 不会重跑 build script，旧图标继续嵌在二进制里。

**最小成本方案**（零秒，触发增量编译）：

```powershell
(Get-Item src-tauri\tauri.conf.json).LastWriteTime = Get-Date
```

然后重启 `pnpm tauri dev` 即可。

**不要用** `cargo clean` — 那会重编译整个依赖树，浪费几分钟。
