# Ky desktop

Ky 是基于 Tauri 2 的本地桌面工作台：文本剪贴板历史、配置片段版本和既有 kyvault 密钥库在同一窗口操作。界面使用原生 HTML/CSS/JavaScript 与系统 WebView，没有前端运行时依赖，也不运行本地 HTTP 服务。

## 在 macOS 构建与使用

需要 Rust stable、Node.js 22+ 和 Xcode Command Line Tools。

```sh
cd desktop
npm ci
npm run build
open src-tauri/target/release/bundle/macos/Ky.app
```

构建生成 `Ky.app` 与 DMG。可将 App 拖到 Applications。当前产物为本机构建，未配置 Developer ID 签名或 Apple 公证，不作为已公证公开发行版。

开发运行 `npm run dev`。关闭窗口会隐藏到托盘；从托盘选择退出才结束进程。`⌘⇧Space`（Windows/Linux 为 Ctrl+Shift+Space）呼出窗口；快捷键被占用时仍可通过 Dock 或托盘打开。首版的打包和实际运行以 macOS 为验证目标。

## 日常操作

- **菜单栏**：Ky 常驻在 macOS 菜单栏。菜单里可以直接查看最近复制的文本，点击即复制；也能从这里开关剪贴板记录、隐藏敏感内容或退出应用。窗口关闭只是隐藏，不结束进程。
- **History**：手动捕获剪贴板，或在设置里开启自动捕获。文本先通过本地规则脱敏，再加密保存。搜索、预览与复制仅使用保留下来的文本。
- **Snippets**：为文本指定名称，每次保存形成一个版本。恢复旧版会新增一个版本，保留之前的记录。
- **Vault**：读取现有密钥引用；新增、替换、删除密钥；复制引用供 Agent 使用。复制密钥原文需明确确认，明文不会回传给前端或写入历史。30 秒后，仅当剪贴板仍是该值时清理。
- **隐私遮罩**：手动隐藏内容或五分钟未操作后隐藏，同时暂停自动捕获。返回工作台无需密码，它不是身份认证；离开电脑请使用系统锁屏。
- **语言**：界面默认中文，可在设置里切换英文。选择会被记住，下次启动沿用。

历史和片段版本合计保留最近 200 条、30 天，每条最多 64 KiB。期限在下一次读取或写入时清理。命名片段同样受此限制，不能替代永久备份。

界面语言偏好保存在本地 `prefs.json`，只记录语言与是否开启自动捕获，不含任何密钥或历史内容。

## 本地存储与边界

历史默认保存在系统应用数据目录 `com.kyvault.desktop` 下，使用独立随机密钥与现有 AES-256-GCM 实现。Unix 目录权限为 0700、文件为 0600；写入使用文件锁和原子替换，没有明文搜索索引。历史不会进入 kyvault 的 Git 同步目录，也不会发送到云端。

密钥管理直接复用 kyvault Rust Store，遵守已有 `KYVAULT_STORE_DIR` / `KEYRING_STORE_DIR` 环境变量和默认目录。Finder 启动的 App 不继承终端临时环境变量；使用自定义库时应通过带该环境变量的启动方式运行。桌面本身不触发 Git 同步。

本地脱敏识别常见 credential 字段、token 前缀和 PEM 块，属于尽力识别，不能保证识别所有未标记密码、个人信息或业务机密。敏感配置可能整行或整段移除；保存前可预览，不保留被移除的原文。隐私遮罩也不能防御已获得当前系统账户访问权的程序；加密密钥与密文在同一用户账户下。

系统剪贴板可被其他应用读取或跨设备同步，定时清理无法撤回其他应用已经保存的副本。自动捕获默认开启（剪贴板历史是本产品的核心）；macOS 每秒检查轻量变更计数，仅在变化时读取文本。纯文本首版不包含 OCR、图片历史、云同步、本地大模型、Git 仓库界面或密钥版本回滚。

## 开发验证

```sh
cd desktop
npm ci
npm run check
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cd ../../..
cd rust
cargo test --all-features
```

测试使用独立目录：`KYVAULT_STORE_DIR` 指定 fixture 密钥库，`KY_DESKTOP_DATA_DIR` 指定 fixture 历史。测试数据只能使用假值。CLI 与桌面共享加密逻辑，桌面只将引用和已脱敏内容返回界面；Tauri 命令按本地 main 窗口显式授权，禁止远程页面和通用文件/命令执行 API。

UI 设计依据见根目录 `DESIGN.md`。图标沿用项目官网的 K 字标和薄荷绿配色，来源及派生过程见 `brand/README.md`。
