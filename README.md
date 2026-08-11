<p align="center">
  <img src="https://cdn.jsdelivr.net/gh/webkubor/picx-images-hosting@master/blog/projects/keyring-banner/cs-token4ai-1784197546810397000.png" alt="Kyvault Banner" width="100%">
</p>

<h1 align="center">🔐 Kyvault</h1>

<p align="center">
  <strong>AI 时代开发者密钥与资产台账管理器 — 你存一次，AI 永远看不到明文。</strong>
</p>

<p align="center">
  <a href="https://github.com/webkubor/kyvault/releases"><img src="https://img.shields.io/github/v/release/webkubor/kyvault?style=for-the-badge&color=coral" alt="Version"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/webkubor/kyvault?style=for-the-badge&color=gold" alt="License"></a>
  <a href="https://pypi.org/project/kyvault/"><img src="https://img.shields.io/pypi/v/kyvault?style=for-the-badge&color=orange" alt="PyPI"></a>
  <a href="https://pypi.org/project/kyvault/"><img src="https://img.shields.io/pypi/dm/kyvault?style=for-the-badge&color=orange" alt="PyPI Downloads"></a>
  <a href="https://www.python.org/"><img src="https://img.shields.io/badge/Python-3.10%2B-blue?style=for-the-badge" alt="Python"></a>
  <a href="https://github.com/astral-sh/uv"><img src="https://img.shields.io/badge/Built%20with-uv-000000?style=for-the-badge" alt="uv"></a>
  <a href="https://github.com/psf/black"><img src="https://img.shields.io/badge/code%20style-black-000000?style=for-the-badge" alt="Black"></a>
</p>

---

## 🎯 为什么需要 Kyvault？ (核心对比)

| 功能 | Kyvault | .env | 1Password | Vault |
|------|---------|------|-----------|-------|
| **AI 安全 (别名注入)** | **✅ 绝对安全** | ❌ 泄漏明文 | ✅ 安全但慢 | ✅ 复杂难用 |
| **时效覆写 (自动防腐)** | **✅ 失效必改** | ❌ 无校验 | ❌ 手动更新 | ❌ 手动更新 |
| **多平台 CLI 智能连接** | **✅ 一键连接** | ❌ 不支持 | ❌ 不支持 | ❌ 不支持 |
| **纯本地存储 (零网络)** | **✅ 极速响应** | ✅ 本地 | ❌ 依赖云端 | ❌ 依赖云端 |
| **多账户多密钥** | **✅ 支持** | ❌ 不支持 | ✅ 支持 | ✅ 支持 |
| **API Key 验证 (含余额)**| **✅ 自动验证** | ❌ 不支持 | ❌ 不支持 | ❌ 不支持 |
| **轻量依赖** | **✅ 极简** | ✅ 极简 | ❌ 庞大 | ❌ 庞大 |

---

## 🔥 一屏特性亮点

* 🔒 **AI 安全别名注入 (AI-Safe)**: AI 只能看到无害别名（如 `github_token`），运行时单向注入，彻底防止密钥在 AI 聊天日志或训练数据中泄露。
* 🤖 **多平台 CLI 智能对接 (AI Connect)**: 一键 `kyconnect`，自动将规则和技能注入 `Gemini/agy/Claude/Codex/Hermes/OpenCode` 规则库。
* 🖥️ **开发者加密台账中心 (Developer Ledger)**: 加密管理服务器账号密码、云服务租金、CLI 客户端多 Profile 凭证令牌，支持 URI 寻址。
* 🛡️ **失效密钥覆写防腐 (Overwrite Policy)**: 拦截 API 401 报错，刚性规定 Agent 必须立刻覆写（Overwrite）修改失效 Key，杜绝保守残留。

---

## ⚡ 30 秒上手

```bash
# 安装并初始化
pip install kyvault && kyi

# 一键连接本地所有 AI 智能体 (Claude/Codex/Hermes/OpenCode/Cursor)
kyconnect

# AI 零明文注入运行
kyr --env GITHUB_TOKEN=secret://github/personal-pat -- git push
```

**快捷别名：** `ky`=kyvault `kyp`=platform `kya`=account `kyk`=key `kyi`=init `kyr`=run `kyconnect`=kyvault connect

---

## 🔥 核心亮点

### 🔒 1. AI 编码原生安全 (AI-Safe Alias Injection)
* **痛点**：传统的 `.env` 文件或内存环境变量会被 Cursor、Claude Code、GitHub Copilot 等 AI 助手读取其上下文，导致密钥直接暴露在 AI 提供商的聊天日志或训练数据中。
* **解法**：`kyvault` 采用**别名映射注入机制**。AI 在代码和提示词中只能看到无害的“别名”（如 `github_token`），而在运行时（Runtime）通过 `kyvault run` 动态且单向地将明文注入子进程。AI 永远接触不到明文，从源头上杜绝了数据泄露。

### 🤖 2. 智能连接，AI 零配置感知 (Zero-Config AI Connect)
* **一键连接**：内置 `kyvault connect` 命令，能自动发现并注入当前机器的全局 Gemini/Claude 规则与当前项目的 `.agents/` 技能文件。
* **IDE 无感对接**：自动识别项目目录并追加安全别名规则到 `.cursorrules` 与 `.copilotinstructions`。AI 智能体在理解您的项目时会“自动学会”使用 `cs secrets`，实现零人工介入的主动安全运维。

### 🖥️ 3. 加密资产台账中心 (Developer Ledger & CLI Multi-Tokens)
* **服务器台账**：将服务器 IP、root 登录密码、云服务商及月度租用成本以第一公民的数据结构集中加密记录，统一支持 `secret://server/<host>/[ip|root-password]` 的 URI 寻址解密。
* **CLI 多账户管理**：支持针对同一个 CLI 工具（如 `studio-cli`、`git`）管理多套 Profile（如主账户、测试账户、部署账户）的 Token，多账户环境一键读取，杜绝身份混淆。

### 🔑 4. 纯本地军事级加密 (Local Military-Grade Encryption)
* **高强度加密**：采用业界公认安全的 **AES-256-GCM**（认证加密），所有数据在写入磁盘前均完成高强度加密。
* **零网络依赖**：100% 纯本地运行，不发起任何外网连接，绝无任何 SaaS 云端数据泄漏或被拖库的潜在风险。密钥完全掌握在您自己手中。

### 🔄 5. 极简无缝迁移 (.env Migration)
* **无感导入**：支持一键导入项目已有的 `.env` 配置文件，并自动匹配最适合 AI 使用的变量别名。
* **支持 Dry-Run**：在实际导入前提供安全预览机制，清晰掌握数据结构变化。

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

### 🤖 AI 智能体一键连接 (AI Connect)

通过全局和项目本地的智能规则，使您本地的 AI 编码助手（如 Cursor、VSCode Copilot、Claude Code 等）能够立即读懂密钥库及别名别称，彻底避免明文泄漏：
```bash
# 一键自动对接全局 Gemini 规则和当前项目下的 .agents/、.cursorrules 和 .copilotinstructions
kyvault connect
```

### 🖥️ 服务器密码与租金台账 (Server Ledger)

以第一公民命令格式加密存储您的所有服务器台账，支持以 `secret://` 的形式让 AI 直接寻址解密：
```bash
# 1. 保存服务器（必填：主机名、IP、root密码；可选：月租成本、云服务商）
kyvault server set my-host 120.46.12.3 rootpwd123 --cost "99元/月" --provider "腾讯云"

# 2. 查询全部台账信息
kyvault server get my-host

# 3. 指定读取单个加密字段（支持 secret:// URI 路由兼容，完美服务 AI）
kyvault server get my-host --field ip            # 输出: 120.46.12.3
kyvault get secret://server/my-host/root-password # 输出: rootpwd123
```

### 🔌 CLI 客户端多 Token 维护 (CLI Multi-Tokens)

用于多账户、多环境切换的 CLI 统一 Token 凭证维护：
```bash
# 1. 为指定 CLI 的不同账户存储 Token
kyvault cli set studio-cli webkubor jwt_token_main
kyvault cli set studio-cli test-user jwt_token_test

# 2. 查询指定 Profile 的加密令牌
kyvault cli get studio-cli webkubor             # 输出: jwt_token_main
kyvault get secret://cli/studio-cli/test-user   # 输出: jwt_token_test
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
| `kyconnect` | `kyvault connect` | AI 智能对接 | `kyconnect` |
| **加密资产台账** | | | |
| - | `kyvault server set` | 存服务器 | `kyvault server set host 1.1.1.1 pw` |
| - | `kyvault server get` | 读服务器 | `kyvault server get host` |
| - | `kyvault server list` | 列服务器 | `kyvault server list` |
| - | `kyvault server delete`| 删服务器 | `kyvault server delete host` |
| - | `kyvault cli set` | 存 CLI Token | `kyvault cli set tool prof token` |
| - | `kyvault cli get` | 读 CLI Token | `kyvault cli get tool prof` |
| - | `kyvault cli list` | 列 CLI Token | `kyvault cli list` |
| - | `kyvault cli delete`| 删 CLI Token | `kyvault cli delete tool prof` |

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
| SiliconFlow | 🧊 | `ky check siliconflow --key sk-xxx` | ✅ |
| 百川 | 🌊 | `ky check baichuan --key xxx` | ✅ |
| 讯飞星火 | ✨ | `ky check spark --key xxx` | ✅ |
| 阿里云百炼 | ☁️ | `ky check aliyun --key xxx`（含余额） | ✅ |

### 开发与运维平台

| 平台 | Logo | 验证 | 别名注入 |
|------|------|------|----------|
| GitHub | 🐙 | `ky check github --key ghp_xxx` | ✅ |
| Cloudflare | 🧡 | `ky check cloudflare --key clouflare_token` | ✅ |

## 🤝 贡献

欢迎贡献！请查看 [CONTRIBUTING.md](CONTRIBUTING.md)。

```bash
# 开发环境
git clone https://github.com/webkubor/kyvault.git
cd kyvault
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
