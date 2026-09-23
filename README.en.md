<p align="center">
  <img src="https://cdn.jsdelivr.net/gh/webkubor/picx-images-hosting@master/blog/projects/keyring-banner/cs-token4ai-1784197546810397000.png" alt="Kyvault Banner" width="100%">
</p>

<h1 align="center">🔐 Kyvault</h1>

<p align="center">
  <strong>A secrets & asset ledger for developers in the AI era — store once, AI never sees plaintext.</strong>
</p>

<p align="center">
  <a href="https://github.com/webkubor/kyvault/releases"><img src="https://img.shields.io/github/v/release/webkubor/kyvault?style=for-the-badge&color=coral" alt="Version"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/webkubor/kyvault?style=for-the-badge&color=gold" alt="License"></a>
  <a href="https://github.com/webkubor/kyvault/releases/latest"><img src="https://img.shields.io/github/downloads/webkubor/kyvault/total?style=for-the-badge&color=orange" alt="Downloads"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-static%20binary-orange?style=for-the-badge" alt="Rust"></a>
  <a href="#-install"><img src="https://img.shields.io/badge/deps-zero%20runtime-4c9a6b?style=for-the-badge" alt="zero runtime deps"></a>
  <a href="https://github.com/astral-sh/uv"><img src="https://img.shields.io/badge/Built%20with-uv-000000?style=for-the-badge" alt="uv"></a>
  <a href="https://github.com/psf/black"><img src="https://img.shields.io/badge/code%20style-black-000000?style=for-the-badge" alt="Black"></a>
</p>

[中文](./README.md)

---

## 🎯 Why Kyvault? (core comparison)

| Feature | Kyvault | .env | 1Password | Vault |
|------|---------|------|-----------|-------|
| **AI safety (alias injection)** | **✅ absolutely safe** | ❌ leaks plaintext | ✅ safe but slow | ✅ complex |
| **Auto-overwrite on expiry** | **✅ expired keys must rotate** | ❌ no validation | ❌ manual | ❌ manual |
| **Multi-platform CLI auto-connect** | **✅ one command** | ❌ not supported | ❌ not supported | ❌ not supported |
| **Pure local (zero network)** | **✅ instant** | ✅ local | ❌ cloud-dependent | ❌ cloud-dependent |
| **Multi-account multi-key** | ✅ | ❌ | ✅ | ✅ |
| **API key validation (incl. balance)** | ✅ auto | ❌ | ❌ | ❌ |
| **Lightweight** | ✅ minimal | ✅ minimal | ❌ heavy | ❌ heavy |

---

## 🔥 Highlights

* 🔒 **AI-safe alias injection**: AI only ever sees harmless names (e.g. `github_token`); runtime injects plaintext one-way into the subprocess. Keys never appear in AI chat logs or training data.
* 🤖 **Multi-platform CLI smart connect (`kyvault connect`)**: One command auto-injects rules and skills into Gemini/agy/Claude/Codex/Hermes/OpenCode rule libraries.
* 🖥️ **Encrypted developer ledger**: Server account passwords, cloud service rent, CLI multi-profile tokens — all first-class encrypted records, URI-addressable.
* 🛡️ **Anti-rot overwrite policy**: Intercepts API 401 errors; rigidly requires agents to immediately overwrite the expired key, refuses to keep stale ones around.

---

## ⚡ 30-second start

```bash
# Install and init
curl -fsSL https://raw.githubusercontent.com/webkubor/kyvault/main/install.sh | bash

# One-key connect to all local AI agents (Claude/Codex/Hermes/OpenCode/Cursor)
kyvault connect

# AI runs with zero plaintext
kyvault run --env GITHUB_TOKEN=secret://github/personal-pat -- git push
```

> **On the "short aliases":** The Python implementation did expose `ky` / `kyi` / `kya` / `kyk` / `kyp` / `kyr` / `kyconnect` short names (via Click decorators). The Rust implementation dropped them — subcommands start from `kyvault`, and **`kyvault init` / `kyvault run` / `kyvault connect` are already the shortest shell-completion-friendly paths**. This is a deliberate trade-off: one fewer name to maintain; re-introducing the short names in Rust would only widen the drift between the two command surfaces. Add your own shell aliases in `~/.zshrc` / `~/.bashrc` if you want them.

---

## 🔥 Core highlights

### 🔒 1. AI-coding-native safety (AI-Safe Alias Injection)

* **Problem**: Traditional `.env` files or in-memory env vars get read by Cursor, Claude Code, GitHub Copilot etc. into their context — keys leak directly into the AI provider's chat logs or training data.
* **Solution**: Kyvault uses **alias-mapped injection**. AI only sees harmless "aliases" (e.g. `github_token`) in code and prompts; at runtime, `kyvault run` dynamically and one-way injects the plaintext into the subprocess. AI never touches the plaintext — leakage is blocked at the source.

### 🤖 2. Smart connect, AI zero-config aware (Zero-Config AI Connect)

* **One-key connect**: Built-in `kyvault connect` discovers and injects the current machine's global Gemini/Claude rules and the current project's `.agents/` skill files.
* **IDE-transparent**: Auto-detects project dir and appends safe-alias rules to `.cursorrules` and `.copilotinstructions`. AI agents "auto-learn" `kyvault` usage when understanding your project — zero-touch active security ops.

### 🖥️ 3. Encrypted asset ledger (Developer Ledger & CLI Multi-Tokens)

* **Server ledger**: Server IPs, root login passwords, cloud providers and monthly rent — first-class encrypted records, URI-addressable as `secret://server/<host>/[ip|root-password]`.
* **CLI multi-account**: One CLI tool (e.g. `studio-cli`, `git`) can have multiple Profiles (main, test, deploy) — multi-account environments read one key, no identity confusion.

### 🔑 4. Pure local military-grade encryption (Local Military-Grade Encryption)

* **High-strength encryption**: Industry-standard **AES-256-GCM** (authenticated encryption); all data encrypted before disk write.
* **Zero network**: 100% pure local; no external connections; zero SaaS cloud leak or scraping risk. Keys stay in your hands.

> **⚠️ `name` is an alias — don't put the key itself there.**
> The `name` in `secret://<platform>/<name>` is **not encrypted** — it's the addressing handle, `list` prints it as-is.
> Putting the key in `name` means "encrypted one copy, plaintext leaked another" — encryption is pointless.
>
> ```bash
> ✅ kyvault set secret://deepseek/api-key -        # readable alias
> ✅ kyvault set secret://github/main-pat -         # use pat-work / pat-personal for multi-account
> ❌ kyvault set secret://deepseek/sk-9dcea1111... - # secret as name
> ```
>
> **As of v2.2.0, `set` does NOT validate the `name`** — the "secret-as-name" pattern above will be accepted as-is, and `list` will print it as-is. The check is purely on the human side, so it's on the redline: feeding `sk-...` / `ghp_...` etc. as `name` to `set` defeats the encryption entirely.
>
> If name validation is added in the future (matching known prefixes like `sk-`/`ghp_`/`glpat-`/`AKIA`/`AIza`), `list` will mask historical entries as `…{last4}` (the last 4 chars of the value — which is already the current format). Until then — guard the input.

### 🔄 5. Minimal-friction `.env` migration

* **Transparent import**: One-key import of an existing `.env`, with auto-matched aliases optimized for AI use.
* **Dry-run support**: Preview the structure change before actually importing.

---

## 📖 Usage guide

### Account management

```bash
# Save account (username + password)
kyvault account set github user@gmail.com mypassword123
kyvault account set github admin@gmail.com adminpass456

# Read password
kyvault account get github user@gmail.com

# List accounts under a platform
kyvault account list github

# Delete account
kyvault account delete github user@gmail.com
```

### Key management

```bash
# Save platform key (API Key, Token, etc.)
kyvault key set github ghp_xxxxxxxxxxxx
kyvault key set openai sk-xxxxxxxxxxxx

# Read key
kyvault key get github ghp_xxxxxxxxxxxx

# List keys under a platform
kyvault key list github

# Delete key
kyvault key delete github ghp_xxxxxxxxxxxx
```

### Platform query

```bash
# List all platforms and summary
kyvault platform

# Detail of a specific platform
kyvault platform github
```

### AI integration

```bash
# Push code
kyvault run --env GITHUB_TOKEN=ghp_xxxxxxxxxxxx -- git push

# Call API
kyvault run --env OPENAI_API_KEY=sk-xxxxxxxxxxxx -- python app.py

# Multiple keys
kyvault run --env TOKEN1=secret1 --env TOKEN2=secret2 -- python script.py
```

### Alias system

```bash
# Create alias (this is what AI sees)
kyvault alias set github_token secret://github/ghp_xxxxxxxxxxxx

# Inject via alias
kyvault run --env GITHUB_TOKEN=github_token -- git push
```

### Migrate from `.env`

```bash
# Preview (no actual import)
kyvault import --file .env --dry-run

# Import everything
kyvault import --file .env

# Only import GitHub-related
kyvault import --file .env --prefix GITHUB_
```

### 🤖 AI agent one-key connect

Global and project-local smart rules so your local AI coding assistants (Cursor, VSCode Copilot, Claude Code, etc.) can immediately understand the vault and alias names — no plaintext leak:

```bash
# Auto-connect global Gemini rules and the project's .agents/, .cursorrules, .copilotinstructions
kyvault connect
```

### 🖥️ Server password & rent ledger

First-class encrypted storage for all your server records, addressable via `secret://` for direct AI lookup:

```bash
# 1. Save server (required: hostname, IP, root password; optional: monthly rent, cloud provider)
kyvault server set my-host 120.46.12.3 rootpwd123 --cost "99元/月" --provider "Tencent Cloud"

# 2. Read full record
kyvault server get my-host

# 3. Read one encrypted field (secret:// URI route-compatible)
kyvault server get my-host --field ip            # output: 120.46.12.3
kyvault get secret://server/my-host/root-password # output: rootpwd123
```

### 🔌 CLI client multi-token maintenance

Unified token maintenance for multi-account / multi-environment CLIs:

```bash
# 1. Store tokens for different accounts of a CLI
kyvault cli set studio-cli webkubor jwt_token_main
kyvault cli set studio-cli test-user jwt_token_test

# 2. Query encrypted token for a profile
kyvault cli get studio-cli webkubor              # output: jwt_token_main
kyvault get secret://cli/studio-cli/test-user   # output: jwt_token_test
```

---

## 📁 Security architecture

```
~/.keyring/
├── master.key       # AES-256 key (chmod 600)
└── secrets.json     # Encrypted accounts/keys (AES-256-GCM)
```

### Storage structure

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

- **Encryption algorithm**: AES-256-GCM (authenticated encryption)
- **Key derivation**: SHA-256
- **Storage**: Pure local, zero network
- **Permissions**: master.key readable only by owner

---

## 📋 Quick command reference

| Alias | Full | Purpose | Example |
|------|------|------|------|
| - | `kyvault init` | Initialize | `kyvault init` |
| **Account management** | | | |
| `kyvault account set` | `kyvault account set` | Store account | `kyvault account set github user@gmail pass` |
| `kyvault account get` | `kyvault account get` | Read password | `kyvault account get github user@gmail` |
| `kyvault account list` | `kyvault account list` | List accounts | `kyvault account list github` |
| `kyvault account delete` | `kyvault account delete` | Delete account | `kyvault account delete github user@gmail` |
| **Key management** | | | |
| `kyvault key set` | `kyvault key set` | Store key | `kyvault key set github ghp_xxx value` |
| `kyvault key get` | `kyvault key get` | Read key | `kyvault key get github ghp_xxx` |
| `kyvault key list` | `kyvault key list` | List keys | `kyvault key list github` |
| `kyvault key delete` | `kyvault key delete` | Delete key | `kyvault key delete github ghp_xxx` |
| **Platform query** | | | |
| - | `kyvault platform` | Platform list | `kyvault platform` |
| `kyvault platform <name>` | `kyvault platform <name>` | Platform detail | `kyvault platform github` |
| **API validation** | | | |
| - | `kyvault check` | Validate key | `kyvault check openai --key sk-xxx` |
| - | `kyvault providers` | Supported platforms | `kyvault providers` |
| **AI integration** | | | |
| - | `kyvault run` | Inject env | `kyvault run --env X=val -- cmd` |
| - | `kyvault connect` | AI smart connect | `kyvault connect` |
| **Encrypted asset ledger** | | | |
| - | `kyvault server set` | Store server | `kyvault server set host 1.1.1.1 pw` |
| - | `kyvault server get` | Read server | `kyvault server get host` |
| - | `kyvault server list` | List servers | `kyvault server list` |
| - | `kyvault server delete`| Delete server | `kyvault server delete host` |
| - | `kyvault cli set` | Store CLI token | `kyvault cli set tool prof token` |
| - | `kyvault cli get` | Read CLI token | `kyvault cli get tool prof` |
| - | `kyvault cli list` | List CLI tokens | `kyvault cli list` |
| - | `kyvault cli delete`| Delete CLI token | `kyvault cli delete tool prof` |
| **GitLab Team Collaboration** | | | |
| - | `kyvault gitlab status` | Vault status | `kyvault gitlab status` |
| - | `kyvault gitlab pull` | Pull latest | `kyvault gitlab pull` |
| - | `kyvault gitlab push` | Commit & push | `kyvault gitlab push` |
| - | `kyvault gitlab sync` | Auto sync (pull+push)| `kyvault gitlab sync` |
| - | `kyvault gitlab setup`| Clone team repo | `kyvault gitlab setup git@gitlab.com:org/vault.git` |
| **Self-check & update** | | | |
| - | `kyvault doctor` | Tool self-check + repair | `kyvault doctor` |
| - | `kyvault update` | Online tool upgrade | `kyvault update` |

---

## 🤖 Compatible platforms

### LLM providers

| Platform | Logo | Verify | Alias inject |
|------|------|------|----------|
| OpenAI | 🟢 | `kyvault check openai --key sk-xxx` | ✅ |
| DeepSeek | 🔵 | `kyvault check deepseek --key sk-xxx` (with balance) | ✅ |
| Zhipu AI | 🟣 | `kyvault check zhipu --key xxx` (with balance) | ✅ |
| Moonshot (Kimi) | 🌙 | `kyvault check moonshot --key sk-xxx` (with balance) | ✅ |
| Anthropic (Claude) | 🟠 | `kyvault check anthropic --key sk-ant-xxx` | ✅ |
| Google Gemini | 💎 | `kyvault check gemini --key xxx` | ✅ |
| Tongyi Qianwen (Qwen) | ☁️ | `kyvault check qwen --key sk-xxx` | ✅ |
| MiniMax | 🔷 | `kyvault check minimax --key xxx` | ✅ |
| ByteDance Doubao | 🫘 | `kyvault check doubao --key xxx` (with balance) | ✅ |
| Groq | ⚡ | `kyvault check groq --key gsk_xxx` | ✅ |
| Together AI | 🤝 | `kyvault check together --key xxx` | ✅ |
| OpenRouter | 🔀 | `kyvault check openrouter --key sk-or-xxx` | ✅ |
| Fireworks AI | 🔥 | `kyvault check fireworks --key xxx` | ✅ |
| SiliconFlow | 🧊 | `kyvault check siliconflow --key sk-xxx` | ✅ |
| Baichuan | 🌊 | `kyvault check baichuan --key xxx` | ✅ |
| iFlytek Spark | ✨ | `kyvault check spark --key xxx` | ✅ |
| Aliyun Bailian | ☁️ | `kyvault check aliyun --key xxx` (with balance) | ✅ |

### Dev & ops platforms

| Platform | Logo | Verify | Alias inject |
|------|------|------|----------|
| GitHub | 🐙 | `kyvault check github --key ghp_xxx` | ✅ |
| Cloudflare | 🧡 | `kyvault check cloudflare --key cloudflare_token` | ✅ |
| GitLab | 🦊 | `kyvault check gitlab --key glpat-xxx` | ✅ |
| Feishu | 🐦 | `kyvault check feishu --key tenant_access_token` | ✅ |

## 🤝 Contributing

Welcome! See [CONTRIBUTING.md](CONTRIBUTING.md).

```bash
# Dev environment
git clone https://github.com/webkubor/kyvault.git
cd kyvault
cd rust
cargo test          # includes legacy-vault compat (decrypts Python 1.x vaults in tests/fixtures)
cargo fmt --check   # CI gate, run locally first
cargo clippy --all-targets -- -D warnings
```

## 📄 License

MIT — see [LICENSE](LICENSE).

## ❓ FAQ

#### Q: I only have a Node environment on my machine — do I need Python?

**A: No.**

Kyvault is a Rust static binary; encryption uses `aes-gcm` (AES-256-GCM), TLS uses `rustls`. **No Python, no Node, no Rust toolchain required** — download and use.

This is deliberate: the vault is the foundation that every credential-needing job depends on, so it can't rely on any single runtime that might break. The previous Python version paid for this lesson — on some machines, the interpreter's TLS validation fails entirely (same cert chain, `openssl verify` says OK, `curl` connects, but Python reports `CERTIFICATE_VERIFY_FAILED`); the key write channel went read-only on the spot.

---

## 🌟 Like it? Show your support!

If Kyvault helps you, drop a **Star ⭐️** on the GitHub repo — every star is the biggest motivation to keep improving!

👉 **[GitHub repo](https://github.com/webkubor/kyvault)**

---

<p align="center">
  Built with 🔐 by <a href="https://github.com/webkubor">webkubor</a>
</p>