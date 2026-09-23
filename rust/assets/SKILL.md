---
name: kyvault-ops
description: 使用 kyvault (或 cs kyvault) 在本地 and 团队 GitLab 环境中端对端管理加密 API Key、服务器资产台账与多 Profile 凭证。支持 Claude、Codex、Gemini 与 Hermes 全自动读写。
---

# 🛡️ Kyvault 密钥与资产台账端对端运维指南 (kyvault-ops)

`kyvault` 是专为开发者与 AI Agent 设计的本地军工级加密密钥与资产台账管理器。采用 AES-256-GCM 纯本地加密与 GitLab 远程密文同步，解密计算完全脱机，零网络依赖。

## 1. 核心操作命令

Agent (Claude / Codex / Gemini / Hermes) 在需要读取或管理密钥时，应直接执行原生命令（或兼容别名 `cs kyvault`）：

```bash
# 1. 查看密钥目录元信息（只出元信息与脱敏 last4，绝不暴露明文）
kyvault list

# 2. 注入子进程环境变量执行命令（首选，零明文回显，不落盘）
kyvault run --env TOKEN=secret://platform/key_name -- <command>

# 3. 读取特定密钥的明文（仅在必须写配置文件等无法注入的环境使用）
kyvault get secret://platform/key_name

# 4. 写入/更新一个密钥（支持从 stdin 读避免暴露在 ps 进程列表）
kyvault set secret://platform/key_name "<value>" --kind "API Key"
echo "<value>" | kyvault set secret://platform/key_name -

# 5. 校验密钥有效性与余额（验证通过自动同步审计元信息至 meta.json）
kyvault check <platform> <key_name>

# 6. 本地主密钥状态与指纹查看（严禁外传，换机迁移导出备份）
kyvault master-key          # 显示路径、0600权限、SHA-256指纹与遮挡值
kyvault master-key --reveal # 显示完整明文（仅供本人导出至 1Password / 离线保险箱）

# 7. 团队 GitLab 密钥库同步与状态
kyvault gitlab status       # 查看本地与远端密文同步状态及 master.key 安全检查
kyvault gitlab sync         # 自动 pull rebase 后 push 推送最新密文
```

---

## 2. 统一资产与台账命名规范 (SSOT Schema)

### A. 平台与 API Key 凭证 (Platform API Keys)
路径格式：`secret://<platform>/<key_name>`
* `secret://deepseek/main` ➡️ DeepSeek API Key
* `secret://openai/coding` ➡️ OpenAI 编程专用 Key
* `secret://github/personal-pat` ➡️ GitHub 个人访问令牌 (PAT)
* `secret://gitlab/deploy-token` ➡️ GitLab 部署令牌

### B. CLI 客户端多 Profile 维护 (CLI Multi-Tokens)
路径格式：`secret://cli/<cli_name>/<profile_name>`
* `secret://cli/feishu/work` ➡️ 飞书 CLI 工作/企业账号 Token
* `secret://cli/feishu/personal` ➡️ 飞书 CLI 个人账号 Token
* `secret://cli/cloudflare/prod` ➡️ Cloudflare 生产环境 API Token
* `secret://cli/studio-cli/webkubor` ➡️ studio-cli 主账号凭证

### C. 服务器与资产台账 (Server Ledger)
路径格式：`secret://server/<hostname_or_project>/<field>`
* `secret://server/<hostname>/ip` ➡️ 服务器公网 IP
* `secret://server/<hostname>/root-password` ➡️ root 密码
* `secret://server/<hostname>/ssh-key` ➡️ SSH 私钥 (如有)
* `secret://server/<hostname>/cost` ➡️ 服务器月度租金成本 (如 `99元/月`)
* `secret://server/<hostname>/provider` ➡️ 服务商 (如 腾讯云/Cloudflare/阿里云)

---

## 3. 安全架构与 master.key 核心规范

```
~/.config/kyvault/store/ (或 ~/.keyring/)
├── master.key       # 根主密钥 (32字节随机Base64, chmod 600, .gitignore 忽略，绝对不上 Git)
├── secrets.json     # AES-256-GCM 密文 (入仓 Git 同步)
├── meta.json        # 审计元信息 (脱敏, 记录更新时间、last4、scopes、组织)
└── .lock            # 跨进程文件排他锁 (防止 Agent 并发写入冲突)
```

1. **什么是 master.key**：
   `master.key` 是生成于本机的 32 字节高强度随机根密钥（44 字符 Base64 编码）。所有的 `secrets.json` 密文均使用它经过 SHA-256 派生出的 AES-256-GCM 密钥加解密。
2. **master.key 铁律**：
   - **绝对不入 Git**：它被 `.gitignore` 严格排除，`kyvault gitlab push` 前会进行强制安全检查。
   - **单机持有**：在新电脑上配置时，需通过带外信道（如 1Password、加密即时通讯）将这把密钥复制到目标机的对应目录中。
   - **本地查看**：随时运行 `kyvault master-key` 审计文件路径、权限与指纹。

---

## 4. Agent 行为守则 (Rule of Thumb)

1. **绝对禁止硬编码**：Agent 严禁将任何 API 密钥、密码、服务器 IP 写入代码文件、日志或 Git 提交。
2. **先 list 后询问**：缺失环境变量时，先调用 `kyvault list` 寻找，有则直接 `kyvault run` 注入；确认没有才向用户询问。
3. **失效就覆写，杜绝保守残留**：一旦检测到密钥库中的某个 Key 失效（如调用接口报 401、Token 过期或校验失败），Agent **必须立刻主动覆写（Overwrite）修改**，严禁留着旧值盲目重试。
4. **多账号动态寻址**：面临飞书、GitHub 等多 Profile 场景，通过 `kyvault run --env TOKEN=secret://cli/<name>/<profile>` 运行时单向注入，绝不污染全局 CLI 配置。
