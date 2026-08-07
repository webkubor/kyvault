---
name: kyvault
version: 1.2.2
description: "轻量级加密密钥管理 — 支持多账户多密钥，AI 安全注入。触发条件: 需要管理密钥、密码、API Token 时触发。触发词: 密钥、secret、token、API key、password、keyring。"
license: MIT
author: webkubor
category: security
platforms: [linux, macos]
metadata:
  openclaw:
    tags: [security, secrets, passwords, encryption, keyring]
    requires:
      python: [>=3.10, cryptography>=41.0]
---

# Keyring

本地加密密钥库 — 人存一次，AI 用别名注入，永远看不到明文。

## 快捷别名

| 别名 | 等价 | 用途 |
|------|------|------|
| `ky` | `kyvault` | 主命令 |
| `kyp` | `keyring platform` | 列出平台 |
| `kya` | `keyring account` | 账户管理 |
| `kyk` | `keyring key` | 密钥管理 |
| `kyi` | `keyring init` | 初始化 |
| `kyr` | `keyring run` | 注入 env |

## 安装

```bash
pip install kyvault
kyi
```

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
  }
}
```

## API Key 验证

```bash
# 验证 key 是否有效
ky check openai --key sk-xxx
ky check deepseek --key sk-xxx
ky check zhipu --key xxx

# 从 keyring 中读取并验证
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
| GitHub | 🐙 | `ky check github` |

## 双模式设计

| 模式 | 命令 | 谁用 | AI 能否读明文 |
|------|------|------|--------------|
| 交互式 | `kyk get github ghp_xxx` | 人 | 能（但不该） |
| 非交互式 | `kyr --env X=ghp_xxx -- cmd` | AI | **不能** |
