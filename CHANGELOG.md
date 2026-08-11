# 更新日志 (Changelog)

所有对 `kyvault` 项目的重大变更都将记录在本文档中。

## [1.3.0] - 2026-08-11

### ✨ 新增功能 (Added)
- **AI 智能体一键连接 (`kyvault connect`)**：支持自动发现本地与全局 AI 编码配置目录（如 Gemini config），一键注入 `kyvault-ops` 技能规范与规则。同时自动在当前工作项目区注入 `.cursorrules` 与 `.copilotinstructions`，使 IDE 编码助手能安全地使用本地密钥别名，明文零泄露。
- **加密服务器台账 (`kyvault server`)**：支持服务器公网 IP、root 密码、云服务商与月租成本的加密存储，并提供第一公民级 CRUD 命令行界面。
- **CLI 客户端多 Profile 令牌管理 (`kyvault cli`)**：支持为同一个 CLI 工具在不同账户名/Profile 下维护不同的 Token，解决多开发环境鉴权证书漂移问题。
- **URI 二级路由映射**：在 `get_secret` 统一入口增加了对 `secret://server/<hostname>/<field>` 与 `secret://cli/<cli_name>/<profile>` 的路由映射。

### ⚙️ 变更 (Changed)
- 将命令行版本号升级为 `1.3.0`。
- 重构了 `store.py` 内部的数据隔离结构，将保留字 `_servers` 与 `_clis` 进行系统级隔离，不影响原扁平平台账户与密钥列出。
