---
name: kyvault-ops
version: 2.6.0
description: 使用 kyvault (或快捷短命令 ky) 在本地与 GitLab 团队环境中端对端管理加密 API Key、服务器资产台账与多 Profile 凭证。支持 Claude、Codex、Gemini 与 Hermes 全自动读写与子进程安全注入。
license: MIT
author: webkubor
category: security
platforms: [linux, macos]
---

# 🛡️ Kyvault 密钥与资产台账端对端运维指南 (kyvault-ops)

`kyvault` 是本地加密凭证台账与 Git 密文同步工具，面向开发者和本地 AI Agent。文件后端使用 AES-256-GCM；GitLab 同步、API 检查和升级会联网。早期 Auth Guard 读取公开 SSH `.pub` 派生 AES key，不验证私钥持有；远程 Agent 的逐条授权由实验性的 `ky broker`（OpenSSH forced-command）承担，主设备签名审批与自动轮换尚未实现。

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

# 4b. 长密钥 / 多行 PEM 走文件，别塞 argv（明文会进 ps 与 shell history）
kyvault set secret://platform/key_name - --file /path/to/key.pem --kind "Private Key"
# 有 BEGIN 没配对的 END 会被拒收（看着像被截断）；确认无误再加 --force

# 5. 校验密钥有效性与余额（验证通过自动同步审计元信息至 meta.json）
kyvault check <platform> <key_name>
kyvault check email <key_name>      # 邮箱授权码 / 应用专用密码：IMAP 登录验证（不是 HTTP API）

kyvault master-key          # 显示路径、0600 权限与脱敏指纹
# 注意：Auth Guard 目前仅为旧版兼容包装；公开 SSH .pub 会被用作派生材料，未验证私钥持有。
# 它不是有效的密库保护或 Agent 身份认证。不要向远程 Agent 传播 master.key。

# 7. Auth Guard 旧版兼容管理（不构成 SSH 私钥保护）
kyvault auth status         # 查看旧版守卫配置；不证明持有 SSH 私钥
# auth setup / auth add 旧格式默认停用；仅兼容迁移时设置 KYVAULT_ALLOW_INSECURE_LEGACY_AUTH=1

# 8. 团队 GitLab 密钥库同步与状态
kyvault gitlab status       # 查看本地与远端密文同步状态及 master.key 安全检查
kyvault gitlab sync         # 自动 pull rebase 后 push 推送最新密文
kyvault gitlab auth         # 当前会用哪把凭证（KY_GITLAB_TOKEN / glab 配置 / glab CLI）
# 本机 glab 已登录时无需再抄 PAT：init --remote 会直接复用 glab 登录态建私有库

# 9. 本地极客 Web GUI 与引导向导
kyvault add                 # 浏览器录入：去空格 → 找旧条目 → 先验证（IMAP / 平台接口），通过才写库
kyvault add --email x@163.com --login   # 邮箱登录密码：两次输入核对；一个邮箱只留一条，新的覆盖旧的
kyvault ui                  # 启动本地暗黑风极客 Web GUI（自动唤起浏览器，地址带一次性随机 token）
kyvault wizard              # 终端交互式分类引导向导（规整化命名）

# 10. 操作审计（谁、什么时候、读过/改过哪条 —— 只记元信息）
kyvault audit                       # 最近 30 条
kyvault audit --action get --limit 50   # 只看读取行为
kyvault audit --actor alice --json  # 按操作人过滤，机器可读
# 操作人默认取 KY_ACTOR → 库内 git user.name → $USER；也可每次 --as <名字>

# 11. 多账户（profile）体系：一个账号 = 一组凭据
kyvault profile add work --platform github --username alice --org acme --link secret://github/pat
kyvault profile list                # 列出账号，标出当前账号
kyvault profile use work            # 切换当前账号
kyvault run --profile work -- npm run deploy   # 整组注入，变量名 = ref 名大写

# 12. 只读托管面板（仅元信息，可放静态托管）
kyvault panel --out panel.html      # 账号 / 条目 / 计数 / 最近审计；无明文、无密文、无 last4
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
