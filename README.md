![logo](https://cdn.jsdelivr.net/gh/webkubor/picx-images-hosting@master/blog/projects/keyring/cs-token4ai-1784193570620950000.png)

![banner](https://cdn.jsdelivr.net/gh/webkubor/picx-images-hosting@master/blog/projects/keyring-banner/cs-token4ai-1784197546810397000.png)

# 🔐 Keyring — AI 时代密钥管理

> **你存一次，AI 永远看不到明文。**

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Python 3.10+](https://img.shields.io/badge/Python-3.10%2B-blue.svg)](https://www.python.org/)
[![Version](https://img.shields.io/badge/Version-1.2.0-blue.svg)](https://github.com/webkubor/kyvault/releases)

---

## ⚡ 30 秒上手

```bash
# 安装
pip install kyvault

# 初始化
kyi

# 存账户密码
kya set github user@gmail.com mypassword

# 存平台密钥
kyk set github ghp_xxxxxxxxxxxx

# AI 用
kyr --env GITHUB_TOKEN=ghp_xxxxxxxxxxxx -- git push
```

**快捷别名：** `ky`=kyvault `kyp`=platform `kya`=account `kyk`=key `kyi`=init `kyr`=run

---

## 🎯 为什么需要 Keyring？

| 方案 | AI 能读明文？ | 安全性 |
|------|--------------|--------|
| `.env` 文件 | ✅ 能 | ❌ 危险 |
| 环境变量 | ✅ 能 | ⚠️ 有风险 |
| **Keyring** | **❌ 不能** | **✅ 安全** |

### 痛点

```
你: 帮我推代码到 GitHub
AI: 好的，我看到你的 token 是 ghp_abc123...（已泄露）
```

### 解决

```
你: 帮我推代码到 GitHub  
AI: kyvault run --env GITHUB_TOKEN=github_token -- git push
    （只看到别名，看不到明文）
```

---

## 🔥 核心亮点

| 特性 | 说明 |
|------|------|
| 🔒 **AI 安全** | 别名注入，明文不暴露 |
| ⚡ **极速** | 纯本地，毫秒级响应 |
| 🎯 **多账户多密钥** | 每个平台支持多个账户和多个密钥 |
| 📦 **轻量依赖** | 仅需 `cryptography>=41.0` |
| 🔄 **兼容** | 支持 .env 导入 |

---

## 📖 使用指南

### 账户管理

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

### 密钥管理

```bash
# 保存平台密钥（API Key、Token 等）
kyk set github ghp_xxxxxxxxxxxx
kyk set openai sk-xxxxxxxxxxxx

# 读取密钥
kyk get github ghp_xxxxxxxxxxxx

# 列出平台下所有密钥
kyk list github

# 删除密钥
kyk delete github ghp_xxxxxxxxxxxx
```

### 平台查询

```bash
# 列出所有平台及摘要
kyp

# 查看指定平台详情
kyp github
```

### AI 集成

```bash
# 推代码
kyr --env GITHUB_TOKEN=ghp_xxxxxxxxxxxx -- git push

# 调 API
kyr --env OPENAI_API_KEY=sk-xxxxxxxxxxxx -- python app.py

# 多个密钥
kyr --env TOKEN1=secret1 --env TOKEN2=secret2 -- python script.py
```

### 别名系统

```bash
# 创建别名（AI 只认识这个）
ky alias set github_token secret://github/ghp_xxxxxxxxxxxx

# 用别名注入
kyr --env GITHUB_TOKEN=github_token -- git push
```

### 从 .env 迁移

```bash
# 预览（不实际导入）
ky import --file .env --dry-run

# 导入全部
ky import --file .env

# 只导入 GitHub 相关
ky import --file .env --prefix GITHUB_
```

---

## 📁 安全架构

```
~/.keyring/
├── master.key       # AES-256 密钥（chmod 600）
└── secrets.json     # 加密后的账户/密钥（AES-256-GCM）
```

### 存储结构

```json
{
  "github": {
    "accounts": {
      "user@gmail": "encrypted_password_1",
      "admin@gmail": "encrypted_password_2"
    },
    "keys": {
      "ghp_xxx": "encrypted_key_1",
      "ghp_yyy": "encrypted_key_2"
    }
  }
}
```

- **加密算法**: AES-256-GCM（认证加密）
- **密钥派生**: SHA-256
- **存储**: 纯本地，零网络
- **权限**: master.key 仅所有者可读

---

## 📋 命令速查

| 快捷 | 完整 | 用途 | 示例 |
|------|------|------|------|
| `kyi` | `kyvault init` | 初始化 | `kyi` |
| **账户管理** | | | |
| `kya set` | `kyvault account set` | 存账户 | `kya set github user@gmail pass` |
| `kya get` | `kyvault account get` | 读密码 | `kya get github user@gmail` |
| `kya list` | `kyvault account list` | 列账户 | `kya list github` |
| `kya delete` | `kyvault account delete` | 删账户 | `kya delete github user@gmail` |
| **密钥管理** | | | |
| `kyk set` | `kyvault key set` | 存密钥 | `kyk set github ghp_xxx value` |
| `kyk get` | `kyvault key get` | 读密钥 | `kyk get github ghp_xxx` |
| `kyk list` | `kyvault key list` | 列密钥 | `kyk list github` |
| `kyk delete` | `kyvault key delete` | 删密钥 | `kyk delete github ghp_xxx` |
| **平台查询** | | | |
| `kyp` | `kyvault platform` | 平台列表 | `kyp` |
| `kyp <name>` | `kyvault platform <name>` | 平台详情 | `kyp github` |
| **API 验证** | | | |
| `ky check` | `kyvault check` | 验证 key | `ky check openai --key sk-xxx` |
| `ky providers` | `kyvault providers` | 支持平台 | `ky providers` |
| **AI 集成** | | | |
| `kyr` | `kyvault run` | 注入env | `kyr --env X=val -- cmd` |

---

## 🤖 兼容平台

### LLM 大模型

| 平台 | Logo | 验证 | 别名注入 |
|------|------|------|----------|
| OpenAI | 🟢 | `ky check openai --key sk-xxx` | ✅ |
| DeepSeek | 🔵 | `ky check deepseek --key sk-xxx`（含余额） | ✅ |
| 智谱 AI | 🟣 | `ky check zhipu --key xxx`（含余额） | ✅ |
| Moonshot (Kimi) | 🌙 | `ky check moonshot --key sk-xxx`（含余额） | ✅ |
| Anthropic (Claude) | 🟠 | `ky check anthropic --key sk-ant-xxx` | ✅ |
| Google Gemini | 💎 | `ky check gemini --key xxx` | ✅ |
| 通义千问 | ☁️ | `ky check qwen --key sk-xxx` | ✅ |
| MiniMax | 🔷 | `ky check minimax --key xxx` | ✅ |
| 字节豆包 | 🫘 | `ky check doubao --key xxx`（含余额） | ✅ |
| Groq | ⚡ | `ky check groq --key gsk_xxx` | ✅ |
| Together AI | 🤝 | `ky check together --key xxx` | ✅ |
| OpenRouter | 🔀 | `ky check openrouter --key sk-or-xxx` | ✅ |
| Fireworks AI | 🔥 | `ky check fireworks --key xxx` | ✅ |
| SiliconFlow | 🧊 | `ky check siliconflow --key sk-xxx` | ✅
| 百川 | 🌊 | `ky check baichuan --key xxx` | ✅ |
| 讯飞星火 | ✨ | `ky check spark --key xxx` | ✅ |
| 阿里云百炼 | ☁️ | `ky check aliyun --key xxx`（含余额） | ✅ |

### 开发平台

| 平台 | Logo | 验证 | 别名注入 |
|------|------|------|----------|
| GitHub | 🐙 | `ky check github --key ghp_xxx` | ✅ |

---

## 🏆 对比

| 功能 | Keyring | .env | 1Password | Vault |
|------|---------|------|-----------|-------|
| AI 安全 | ✅ | ❌ | ✅ | ✅ |
| 本地存储 | ✅ | ✅ | ❌ | ❌ |
| 多账户多密钥 | ✅ | ❌ | ✅ | ✅ |
| API Key 验证 | ✅ | ❌ | ❌ | ❌ |
| 轻量依赖 | ✅ | ✅ | ❌ | ❌ |
| 免费开源 | ✅ | ✅ | ❌ | ⚠️ |
| 命令行 | ✅ | ✅ | ⚠️ | ⚠️ |

---

## 🤝 贡献

欢迎贡献！请查看 [CONTRIBUTING.md](CONTRIBUTING.md)。

```bash
# 开发环境
git clone https://github.com/webkubor/kyvault.git
cd keyring
pip install -e .
pip install pytest
pytest
```

---

## 📄 许可证

[MIT License](LICENSE)

---

<p align="center">
  Built with 🔐 by <a href="https://github.com/webkubor">webkubor</a>
</p>
