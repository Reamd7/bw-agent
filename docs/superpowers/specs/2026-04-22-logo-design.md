## bw-agent Logo Design Spec
**Date**: 2026-04-22

### 1. 核心概念
极简、高对比度的授权代理（Vault）图标。基于方块底座与内嵌的"保险箱转盘+锁孔"设计，象征受控访问和安全托管。

### 2. 视觉结构
- **外框**：圆角矩形（`rx=16`，尺寸 `80x80`），填充 TailwindCSS `indigo-600`
- **内框**：圆角矩形（`rx=11`，尺寸 `68x68`），浅色模式为白色 `#FFFFFF`，深色模式为深灰黑 `#1C2127`
- **转盘外圈**：圆形（`r=20`），填充 `indigo-600`，上下左右四个方向各有一个内凹的圆角矩形切口（`7x8`）
- **转盘内圈**：圆形（`r=11`），填充内框底色（白/深灰）
- **核心锁孔**：中心圆（`r=3.5`）加梯形下摆，构成经典锁孔形状，填充 `indigo-600`

### 3. 色彩规范
使用项目 TailwindCSS 配置的主色调（Indigo 系）：
- **Primary / Main Brand**: `#4f46e5` (Tailwind `indigo-600`)
- **Light Mode Background**: `#FFFFFF`
- **Dark Mode Background**: `#1C2127` (配合 `bg-gray-900`)

### 4. 输出要求
所有图标导出为无背景色的透明 SVG/PNG。
文件清单：
1. `src-tauri/icons/icon.svg` (主矢量图)
2. `src-tauri/icons/32x32.png`
3. `src-tauri/icons/128x128.png`
4. `src-tauri/icons/128x128@2x.png`
5. `src-tauri/icons/icon.ico` (Windows 包含 16/32/48/64/128/256 尺寸)
6. `src-tauri/icons/icon.icns` (macOS 包含多尺寸)

### 5. SVG 源码基准
```xml
<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100" fill="none">
  <!-- 外框 (Indigo-600) -->
  <rect x="10" y="10" width="80" height="80" rx="16" fill="#4f46e5" />
  <!-- 内底 (White/Dark) -->
  <rect x="16" y="16" width="68" height="68" rx="11" fill="#FFFFFF" />
  <!-- 转盘主盘 (Indigo-600) -->
  <circle cx="50" cy="50" r="20" fill="#4f46e5" />
  <!-- 4 个切口 (White/Dark) -->
  <rect x="46.5" y="26" width="7" height="8" rx="2" fill="#FFFFFF" />
  <rect x="46.5" y="66" width="7" height="8" rx="2" fill="#FFFFFF" />
  <rect x="26" y="46.5" width="8" height="7" rx="2" fill="#FFFFFF" />
  <rect x="66" y="46.5" width="8" height="7" rx="2" fill="#FFFFFF" />
  <!-- 转盘内圈 (White/Dark) -->
  <circle cx="50" cy="50" r="11" fill="#FFFFFF" />
  <!-- 中心锁孔 (Indigo-600) -->
  <circle cx="50" cy="48" r="3.5" fill="#4f46e5" />
  <path d="M48 51 L48.5 56 H51.5 L52 51 Z" fill="#4f46e5" />
</svg>
```