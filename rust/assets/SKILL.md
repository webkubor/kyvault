---
name: kyvault-ops
version: 2.7.0
description: 使用 kyvault (或快捷短命令 ky) 在本地与 GitLab 团队环境中端对端管理加密 API Key、服务器资产台账与多 Profile 凭证。支持 Claude、Codex、Gemini 与 Hermes 全自动读写与子进程安全注入。
license: MIT
author: webkubor
category: security
platforms: [linux, macos]
---

# 🛡️ Kyvault 密钥与资产台账端对端运维指南 (kyvault-ops)

`kyvault` 是本地加密凭证台账与 Git 密文同步工具，面向开发者和本地 AI Agent。文件后端使用 AES-256-GCM；GitLab 同步、API 检查与升级会联网。早期 Auth Guard 读取公开 SSH `.pub` 文件派生 AES key，不验证私钥持有，不能作为有效的密库保护或远程 Agent 身份认证。逐条 Agent 授权由实验性的 `ky broker`（OpenSSH forced-command）承担，主设备签名审批与自动轮换尚未实现。

## 1. 核心操作命令

Agent (Claude / Codex / Gemini / Hermes) 在需要读取或管理密钥时，应直接执行原生命令（或快捷短命令 `ky`）：

```bash
# 1. 查看密钥目录元信息（只出元信息与脱敏 last4，绝不暴露明文）
kyvault list                                           # 或 ky list

# 2. 注入子进程环境变量执行命令（首选，零明文回显，不落盘）
kyvault run --env TOKEN=secret://platform/key_name -- <command>

# 3. 读取特定密钥的明文（仅在必须写配置文件等无法注入的环境使用）
kyvault get secret://platform/key_name

# 4. 写入/更新一个密钥（支持从 stdin 读避免暴露在 ps 进程列表）
kyvault set secret://platform/key_name "<value>" --kind "API Key"
echo "<value>" | kyvault set secret://platform/key_name -

# 5. 校验密钥有效性与余额（验证通过自动同步审计元信息至 meta.json）
kyvault check <platform> <key_name>
kyvault check email <key_name>      # 邮箱授权码 / 应用专用密码：IMAP 登录验证（不是 HTTP API）

# 6. 本地主密钥状态
kyvault master-key          # 显示路径、权限与脱敏指纹
# master.key 是整库根凭据；不要复制到远程 Agent。旧 Auth Guard 的 SSH 公钥派生实现不提供私钥保护。

# 7. 旧版 Auth Guard 状态（默认禁止新建旧格式）
kyvault auth status         # 查看旧格式配置；不代表已认证 SSH 私钥持有人
# 兼容迁移才设置 KYVAULT_ALLOW_INSECURE_LEGACY_AUTH=1

# 8. 团队 GitLab 密钥库同步与状态
kyvault gitlab status       # 查看本地与远端密文同步状态及 master.key 安全检查
kyvault gitlab sync         # 自动 pull rebase 后 push 推送最新密文

# 9. 本地极客 Web GUI 与引导向导
kyvault add                 # 浏览器录入：去空格 → 找旧条目 → 先验证（IMAP / 平台接口），通过才写库
kyvault add --email x@163.com --login   # 邮箱登录密码：两次输入核对；一个邮箱只留一条，新的覆盖旧的
kyvault ui                  # 启动本地暗黑风极客 Web GUI（自动唤起浏览器，地址带一次性随机 token）
kyvault wizard              # 终端交互式分类引导向导（规整化命名）
```

---

## 2. 统一资产与台账命名规范 (SSOT Schema)

### A. 平台与 API Key 凭证 (Platform API Keys)
路径格式：`secret://<platform>/<key_name>`
* `secret://deepseek/main` ➡️ DeepSeek API Key
* `secret://openai/coding` ➡️ OpenAI 编程专用 Key
* `secret://github/personal-pat` ➡️ GitHub 个人访问令牌 (PAT)
* `secret://gitlab/deploy-token` ➡️ GitLab 部署令牌
* `secret://cloudflare/api-token` ➡️ Cloudflare API Token

### B. 账号与密码凭据 (Accounts)
路径格式：`secret://account/<platform>/<username>`
* `secret://account/github/webkubor@gmail.com` ➡️ GitHub 密码
* `secret://account/apple/webkubor@163.com` ➡️ Apple ID 密码

### C. 服务器与资产台账 (Server Ledger)
路径格式：`secret://server/<host>/[root-password|ip|port]`
* `secret://server/prod-hk/root-password` ➡️ 香港生产机 SSH 密码
* `secret://server/gpu-box/ip` ➡️ 局域网 GPU 服务器 IP

### D. CLI 客户端多 Profile 凭证 (CLI Multi-Tokens)
路径格式：`secret://cli/<cli_name>/<profile>`
* `secret://cli/gh/main` ➡️ GitHub CLI 主账号 Token
* `secret://cli/studio-cli/test-user` ➡️ 创作中台测试租户凭证

---

## 3. Agent 行为刚性准则 (Golden Rules)

1. **先查库，不问用户**：遇到任何需要 Token、API Key、密码的场景，先执行 `kyvault list`。库里有就直接用；确认没有才开口问用户。
2. **优先注入，不取明文**：优先使用 `kyvault run --env TOKEN=secret://... -- <cmd>` 单向注入子进程环境变量，绝不回显明文。
3. **失效必改，刚性覆写**：遇到 401 或 Token 过期，立即主动使用 `kyvault set` 覆写新值，严禁保留失效旧凭据或盲目重试。
4. **master.key 绝不上云**：`master.key` 与 `master.key.enc` 是解密根凭据，绝对禁止暂存或提交到 Git 仓库。
