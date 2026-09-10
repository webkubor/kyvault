# 更新日志 (Changelog)

所有对 `kyvault` 项目的重大变更都将记录在本文档中。

## [2.0.0] - 2026-09-10

### 💥 破坏性变更 (Breaking)
- **实现语言换成 Rust，分发换成静态二进制。** 安装方式从 `pip install kyvault`（那条命令其实一直跑不通，见下）改为 `curl -fsSL .../install.sh | bash`，不再需要 Python、也不需要 Rust 工具链。
- **不再发布到 PyPI。** 历史上 `publish.yml` 每次打 tag 都以 `invalid-publisher: valid token, but no corresponding publisher` 失败——PyPI 侧从未配置 trusted publisher，所以包一次都没上传成功，而 README 的第一条命令一直写着 `pip install kyvault`。访客照着第一步走就是 404。这次不补发 PyPI，而是从根上换掉分发方式：Rust 版存在的意义就是不依赖运行时，再挂一条 pip 链条自相矛盾。

### ✨ 为什么重写
不是为了性能，是**消除 Python 环境这个单点故障**。密钥库是所有需要凭据的工作的底座，它只读就等于全线停工；而 1.3.0 那条 curl 兜底修的正是「解释器 TLS 验证整体失效」——那是在给一个不该存在的依赖打补丁。Rust 走 `rustls` 静态链接，根本不碰系统 OpenSSL 那条路径。

### 🔒 数据兼容（存量密钥原地可读，无需迁移）
两套 wire format 完整实现并做成自动化测试：

| | 本地 file 后端 | Cloudflare D1 后端 |
|---|---|---|
| 密钥派生 | `SHA256(base64_decode(master.key))` | `base64_decode(site_config)` 直接用，不派生 |
| 密文布局 | `base64(nonce ‖ ciphertext)` 单字符串 | ciphertext / nonce 分两列各自 base64 |

`tests/interop.rs` 真的起 python3 做双向对照，两套格式各测一遍（Python 加密 → Rust 解得开；Rust 加密 → Python 解得开），再加一条「两套 key 派生不可互换」防止有人图省事把两个函数合并。

实测：读真实 D1 库 162 条、字段名与 CortexOS Go 端完全对齐；同一条密钥两边解密结果 sha256 前 16 位一致、长度一致。

### ✅ 已实现
`init` / `get` / `set`（含 `-` 从 stdin 读，密钥不进 argv）/ `delete` / `annotate` / `list --json` / `platforms` / `run` / `alias` / `account` / `key` / `platform`。

`cs kyvault` 的 D1 直连是 exec 调用本 CLI，它依赖的 5 个入口与输出格式全部对齐，无需改动 CortexOS。

### ⏳ 尚未移植
`import`（.env 导入）、`check` / `providers`（各家 API 校验）、`connect`（kyconnect）。需要这三个的场景暂时仍可用 Python 实现（仓库内保留）。`wizard` 不打算移植——交互式向导要人对着终端敲，与「人只做决策、agent 执行」冲突。

### 🐛 修复 (Fixed)

### 🐛 修复 (Fixed)
- **D1 后端在 Python TLS 失效的机器上不再整个不可用**：`_query` 改为 urllib 优先、传输层失败时自动回退 `curl`。触发它的是一类真实故障——证书链、CA bundle、系统时间全部正常，同一条链用 `openssl verify` 判 OK、`curl` 也连得通，唯独 Python 报 `CERTIFICATE_VERIFY_FAILED`，且跨解释器复现（homebrew python 3.10/3.13/3.14 与 uv 独立构建 3.11/3.12 全中，只有链 LibreSSL 的系统 python 幸免），换版本、重装 openssl、换 CA bundle 均无效。密钥写入是刚需，不能因为解释器的 TLS 坏了就完全写不进去。
  - 回退路径不降低安全性：token 走 `--config` 临时文件（0600）而非 argv（避免 `ps` 泄露），payload 走 stdin 不落盘，且**绝不使用** `-k/--insecure`——换掉的是 HTTP 客户端，不是 TLS 验证。
  - 仅传输层异常才回退；HTTP 4xx/5xx 仍按原样交给 `success` 判断，免得把业务错误伪装成网络问题。

## [1.3.0] - 2026-08-11

### ✨ 新增功能 (Added)
- **AI 智能体一键连接 (`kyvault connect`)**：支持自动发现本地与全局 AI 编码配置目录（如 Gemini config），一键注入 `kyvault-ops` 技能规范与规则。同时自动在当前工作项目区注入 `.cursorrules` 与 `.copilotinstructions`，使 IDE 编码助手能安全地使用本地密钥别名，明文零泄露。
- **加密服务器台账 (`kyvault server`)**：支持服务器公网 IP、root 密码、云服务商与月租成本的加密存储，并提供第一公民级 CRUD 命令行界面。
- **CLI 客户端多 Profile 令牌管理 (`kyvault cli`)**：支持为同一个 CLI 工具在不同账户名/Profile 下维护不同的 Token，解决多开发环境鉴权证书漂移问题。
- **URI 二级路由映射**：在 `get_secret` 统一入口增加了对 `secret://server/<hostname>/<field>` 与 `secret://cli/<cli_name>/<profile>` 的路由映射。

### ⚙️ 变更 (Changed)
- 将命令行版本号升级为 `1.3.0`。
- 重构了 `store.py` 内部的数据隔离结构，将保留字 `_servers` 与 `_clis` 进行系统级隔离，不影响原扁平平台账户与密钥列出。
