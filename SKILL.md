---
name: kyvault
version: 1.4.0
description: "轻量级加密密钥管理 — 支持多账户多密钥，AI 安全注入。触发条件: 需要管理密钥、密码、API Token、服务器台账或 CLI 令牌时触发。触发词: 密钥、secret、token、API key、password、kyvault、server ledger、cli token。"
license: MIT
author: webkubor
category: security
platforms: [linux, macos]
metadata:
  openclaw:
    tags: [security, secrets, passwords, encryption, kyvault]
    requires:
      python: [">=3.10", "cryptography>=41.0"]
---

# Kyvault

本地加密密钥库 — 人存一次，AI 用别名注入，永远看不到明文。

## 快捷别名

| 别名 | 等价 | 用途 |
|------|------|------|
| `ky` | `kyvault` | 主命令 |
| `kyp` | `kyvault platform` | 列出平台 |
| `kya` | `kyvault account` | 账户管理 |
| `kyk` | `kyvault key` | 密钥管理 |
| `kyi` | `kyvault init` | 初始化 |
| `kyr` | `kyvault run` | 注入 env |
| `kyconnect` | `kyvault connect` | AI 一键连接 |
| - | `kyvault doctor` | 工具自检修复 |
| - | `kyvault update` | 在线升级工具 |

## 安装与对接

```bash
curl -fsSL https://raw.githubusercontent.com/webkubor/kyvault/main/install.sh | bash
kyvault init          # 生成 master key（已存在则原样返回，绝不覆盖）
```

`install.sh` 装的是 Rust 静态二进制到 `~/.local/bin`，并顺手清掉旧的 Python 版
（pipx 包 / `/usr/local/bin` 里的 python shim / 数据目录下的模块副本）。
**不要用 `pip install kyvault`** —— PyPI 从来没有成功发布过，那条指引一直是错的。

## 账户管理

```bash
# 保存账户（用户名+密码）
kya set github user@gmail.com mypassword123
kya set github admin@gmail.com adminpass456

# 读取密码
kya get github user@gmail.com

# 列出平台下所有账户
kya list github

# 删除账户
kya delete github user@gmail.com
```

## 密钥管理

```bash
# 保存平台密钥（API Key、Token 等）
kyk set github ghp_xxxxxxxxxxxx mytoken
kyk set openai sk-xxxxxxxxxxxx mykey

# 读取密钥
kyk get github ghp_xxxxxxxxxxxx

# 列出平台下所有密钥
kyk list github

# 删除密钥
kyk delete github ghp_xxxxxxxxxxxx
```

## 平台查询

```bash
# 列出所有平台及摘要
kyp

# 查看指定平台详情
kyp github
```

## AI 集成

```bash
# 推代码
kyr --env GITHUB_TOKEN=ghp_xxxxxxxxxxxx -- git push

# 调 API
kyr --env OPENAI_API_KEY=sk-xxxxxxxxxxxx -- python app.py

# 多个密钥
kyr --env TOKEN1=secret1 --env TOKEN2=secret2 -- python script.py
```

## 别名系统

```bash
# 创建别名（AI 只认识这个）
ky alias set github_token secret://github/ghp_xxxxxxxxxxxx

# 用别名注入
kyr --env GITHUB_TOKEN=github_token -- git push
```

## 从 .env 迁移

```bash
# 预览（不实际导入）
ky import --file .env --dry-run

# 导入全部
ky import --file .env

# 只导入 GitHub 相关
ky import --file .env --prefix GITHUB_
```

## 🖥️ 服务器密码与租金台账 (Server Ledger)

```bash
# 保存服务器
kyvault server set my-host 120.46.12.3 rootpwd123 --cost "99元/月" --provider "腾讯云"

# 查询全部台账信息
kyvault server get my-host

# 指定读取单个加密字段
kyvault server get my-host --field ip
kyvault get secret://server/my-host/root-password
```

## 🔌 CLI 客户端多 Token 维护 (CLI Multi-Tokens)

```bash
# 为指定 CLI 的不同账户存储 Token
kyvault cli set studio-cli webkubor jwt_token_main
kyvault cli set studio-cli test-user jwt_token_test

# 查询指定 Profile 的加密令牌
kyvault cli get studio-cli webkubor
kyvault get secret://cli/studio-cli/test-user
```

## 存储结构

```
~/.keyring/
├── master.key       # AES-256 密钥（chmod 600）
└── secrets.json     # 加密后的账户/密钥
```

```json
{
  "github": {
    "accounts": {
      "user@gmail.com": "encrypted_password",
      "admin@gmail.com": "encrypted_password"
    },
    "keys": {
      "ghp_xxx": "encrypted_key",
      "ghp_yyy": "encrypted_key"
    }
  },
  "_servers": {
    "my-host": {
      "ip": "encrypted_ip",
      "root-password": "encrypted_password",
      "cost": "encrypted_cost",
      "provider": "encrypted_provider"
    }
  },
  "_clis": {
    "studio-cli": {
      "webkubor": "encrypted_token"
    }
  }
}
```

## API Key 验证

```bash
# 验证 key 是否有效
ky check openai --key sk-xxx
ky check deepseek --key sk-xxx
ky check zhipu --key xxx

# 从 kyvault 中读取并验证
kyk set openai sk-xxx mykey
ky check openai mykey

# 查看支持的平台
ky providers
```

### 支持的平台

| 平台 | Logo | 命令 |
|------|------|------|
| OpenAI | 🟢 | `ky check openai` |
| DeepSeek | 🔵 | `ky check deepseek`（含余额） |
| 智谱 AI | 🟣 | `ky check zhipu`（含余额） |
| Moonshot | 🌙 | `ky check moonshot`（含余额） |
| Anthropic | 🟠 | `ky check anthropic` |
| Gemini | 💎 | `ky check gemini` |
| 通义千问 | ☁️ | `ky check qwen` |
| 阿里云百炼 | ☁️ | `ky check aliyun`（含余额） |
| MiniMax | 🔷 | `ky check minimax` |
| 字节豆包 | 🫘 | `ky check doubao`（含余额） |
| Groq | ⚡ | `ky check groq` |
| OpenRouter | 🔀 | `ky check openrouter` |
| SiliconFlow | 🧊 | `ky check siliconflow` |
| GitHub | 🐙 | `ky check github` |
| Cloudflare | 🧡 | `ky check cloudflare` |
| GitLab | 🦊 | `ky check gitlab` |
| Feishu | 🐦 | `ky check feishu` |

## 双模式设计

| 模式 | 命令 | 谁用 | AI 能否读明文 |
|------|------|------|--------------|
| 交互式 | `kyk get github ghp_xxx` | 人 | 能（但不该） |
| 非交互式 | `kyr --env X=ghp_xxx -- cmd` | AI | **不能** |

## 多 agent 环境：谁能读哪条

一个密钥库被多个 AI agent 共用时（本机 Mac + 若干远程服务器），**不是每个 agent 都该
读到全部密钥**。这就是 `visibility` 的用途 —— 它标记「谁能读这一条」。

```bash
# 仅本机环境可读（远程 agent 一律拿不到）
kyvault annotate secret://cloudflare/api-token --visibility local

# 仅点名的 agent 可读
kyvault annotate secret://feishu/nanzhu-app-id --visibility "agent:nanzhu,vex"

# 改回不限（空串是显式清空，跟不给这个 flag 是两回事）
kyvault annotate secret://github/pat --visibility ""
```

| 取值 | 含义 |
|------|------|
| 空 / `all` | 不限（默认，收紧是逐条做的） |
| `local` | 仅本机环境可读 |
| `agent:a,b` | 仅点名的 agent 可读 |

### 三条会影响你判断的行为

1. **越权读返回 404，不是 403。** 服务端故意让「没有这条」和「你没有权限」返回同一个
   响应 —— `403` 会确认这条密钥确实存在，等于把密钥库目录告诉了撞名字的人。
   **所以取不到一条密钥时，不要断定它不存在**，先用 `cs kyvault whoami` 确认自己的边界。

2. **远程 agent 不能写。** 写入/删除一律 `403 Forbidden: write requires local environment`。
   轮换密钥只能在本机做 —— 远程机器能改密钥库意味着被拿下之后可以投毒。

3. **本机环境一律放行。** 机器是所有者本人的，本机上跑什么形态
   （WorkBuddy / Claude Code / Codex / agy）都在这条信任边界内。
   `visibility` 约束的**只是远程机器上的 agent**，不会把所有者关在门外。

### 拿 key 之前先问一句

```bash
cs kyvault whoami
# 身份:     nanzhu
# 环境:     server — 远程环境，按白名单
# 可写:     否
# 可读:     111 / 169 条
```

远程 agent 尤其需要这个：越权读一律 404，没有这条命令就只能靠撞墙来学自己的边界，
每一次学习都是一条噪音审计记录。
