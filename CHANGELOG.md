# 更新日志 (Changelog)

所有对 `kyvault` 项目的重大变更都将记录在本文档中。

## [未发布]

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
