<p align="center">
  <img src="https://cdn.jsdelivr.net/gh/webkubor/picx-images-hosting@master/blog/projects/keyring-banner/cs-token4ai-1784197546810397000.png" alt="Kyvault Banner" width="100%">
</p>

<h1 align="center">🔐 Kyvault</h1>

<p align="center">
  <strong>A secrets & asset ledger for developers in the AI era — inject by alias to reduce plaintext exposure in model context.</strong>
</p>

<p align="center">
  <a href="https://github.com/webkubor/kyvault/releases"><img src="https://img.shields.io/github/v/release/webkubor/kyvault?style=for-the-badge&color=coral" alt="Version"></a>
  <a href="https://crates.io/crates/kyvault"><img src="https://img.shields.io/crates/v/kyvault?style=for-the-badge&color=orange" alt="crates.io"></a>
  <a href="https://github.com/webkubor/kyvault/actions/workflows/test.yml"><img src="https://img.shields.io/github/actions/workflow/status/webkubor/kyvault/test.yml?style=for-the-badge&color=4c9a6b" alt="Test"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/webkubor/kyvault?style=for-the-badge&color=gold" alt="License"></a>
  <a href="https://github.com/webkubor/kyvault/releases/latest"><img src="https://img.shields.io/github/downloads/webkubor/kyvault/total?style=for-the-badge&color=orange" alt="Downloads"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-prebuilt%20binary-orange?style=for-the-badge" alt="Rust"></a>
  <a href="#-30-second-start"><img src="https://img.shields.io/badge/deps-no%20language%20runtime-4c9a6b?style=for-the-badge" alt="no additional language runtime"></a>
</p>

<p align="center">
  <strong>A local-first secret manager — API key manager, token manager and password manager in one binary.</strong><br>
  It keeps every credential you own: API keys, tokens, passwords, server root logins and CLI profiles.<br>
  AES-256-GCM encryption provides local secret management and a .env alternative; `ky run` injects credentials into subprocesses so you can keep plaintext out of model context.
</p>

[中文](./README.md)

---

## 🎯 Kyvault behavior and boundaries

| Capability | Actual behavior and boundary |
|------|----------------|
| Local storage by default | The file backend uses AES-256-GCM encryption; reading and decryption need no network access. |
| Alias injection with `ky run` | Commands reference aliases or `secret://` URIs; values enter the child process environment at runtime without first passing through model context. |
| Plaintext reads with `ky get` | Prints the decrypted value to the terminal; an AI assistant running it may receive that output in its context. |
| Optional network features | API validation and balance checks, updates, GitLab sync, and the optional D1 backend access the network. |
| Child process output | A child process can read and print injected values; Kyvault does not sandbox it or automatically filter its logs. |

---

## 🔥 Highlights

* 🔒 **Alias injection**: Reference credentials by name (e.g. `github_token`); `ky run` injects values into the child environment at runtime without putting them in prompts. The child can still read or print those values.
* 🤖 **Multi-platform CLI smart connect (`kyvault connect`)**: One command auto-injects rules and skills into Gemini/agy/Claude/Codex/Hermes and project-local rule files.
* 🖥️ **Encrypted developer ledger**: Server account passwords, cloud service rent, CLI multi-profile tokens — all first-class encrypted records, URI-addressable.
* 🛡️ **Credential maintenance guidance**: Agent rules instruct assistants to replace invalid credentials after a validation failure; Kyvault does not automatically intercept every child process API error.

---

## ⚡ 30-second start

```bash
# Install and init
curl -fsSL https://raw.githubusercontent.com/webkubor/kyvault/main/install.sh | bash

# or, with a Rust toolchain
cargo install kyvault

# Add rules for supported local AI assistants (Claude/Codex/Hermes/Cursor)
kyvault connect

# Construct commands with references; avoid printing secrets from the child
kyvault run --env GITHUB_TOKEN=secret://github/personal-pat -- git push
```

> **Short command `ky`:** The installer creates a `ky` symlink to `kyvault`. Both commands use the same binary and subcommands: `ky init`, `ky run`, and `ky connect` are equivalent to their `kyvault` forms.

---

## 🔥 Core highlights

### 🔒 1. AI-coding-native safety (AI-Safe Alias Injection)

* **Problem**: When an AI assistant reads a credential-bearing `.env` file or command output containing environment variables, plaintext may enter model context and session records. Training use depends on the service's policies and settings.
* **Solution**: With **alias-mapped injection**, an assistant constructs a command using an alias or `secret://` URI, then `ky run` passes the value to the child environment. This reduces the need to hand plaintext to the model; it is not a sandbox, and `ky get` or child output can still expose credentials.

### 🤖 2. Smart connect, AI zero-config aware (Zero-Config AI Connect)

* **One-key connect**: Built-in `kyvault connect` discovers and injects the current machine's global Gemini/Claude rules and the current project's `.agents/` skill files.
* **IDE-transparent**: Auto-detects project dir and appends safe-alias rules to `.cursorrules` and `.copilotinstructions`. AI agents "auto-learn" `kyvault` usage when understanding your project — zero-touch active security ops.

### 🖥️ 3. Encrypted asset ledger (Developer Ledger & CLI Multi-Tokens)

* **Server ledger**: Server IPs, root login passwords, cloud providers and monthly rent — first-class encrypted records, URI-addressable as `secret://server/<host>/[ip|root-password]`.
* **CLI multi-account**: One CLI tool (e.g. `studio-cli`, `git`) can have multiple Profiles (main, test, deploy) — multi-account environments read one key, no identity confusion.

### 🔑 4. Local encryption

* **Authenticated encryption**: The file backend uses **AES-256-GCM** for credential values; addressing names and some audit metadata are not encrypted.
* **Offline reads**: Reading and decryption with the default file backend need no network access. API validation, updates, GitLab sync, and the optional D1 backend access the network.

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
kyvault run --env GITHUB_TOKEN=secret://github/pat -- git push

# Call API
kyvault run --env OPENAI_API_KEY=secret://openai/api-key -- python app.py

# Multiple keys
kyvault run --env TOKEN1=secret://example/first --env TOKEN2=secret://example/second -- python script.py
```

### Alias system

```bash
# Create alias (this is what AI sees)
kyvault alias set github_token secret://github/pat

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

Global and project-local rules explain the vault and alias workflow to AI coding assistants (Cursor, VSCode Copilot, Claude Code, etc.) and guide them to prefer injection commands:

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
- **Storage**: Local file backend by default; optional GitLab ciphertext sync or D1 backend
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

Kyvault provides prebuilt Rust binaries; encryption uses `aes-gcm` (AES-256-GCM), TLS uses `rustls`. **Prebuilt versions need no Python, Node, or Rust toolchain**. Linux release targets use the GNU ABI and still require compatible system libraries.

This is deliberate: the vault is the foundation that every credential-needing job depends on, so it can't rely on any single runtime that might break. The previous Python version paid for this lesson — on some machines, the interpreter's TLS validation fails entirely (same cert chain, `openssl verify` says OK, `curl` connects, but Python reports `CERTIFICATE_VERIFY_FAILED`); the key write channel went read-only on the spot.

---

## 🔎 Find us by

Looking for a dotenv / `.env` replacement, or a CLI tool that keeps API keys, tokens, passwords and
server credentials in one encrypted store? That is what Kyvault is. Existing `.env` files import with
`kyvault import --file .env`, and from then on `ky run` hands values to a subprocess without putting
them in your prompt or your model's context.

It is local-first: the default file backend runs locally and decrypts without network access.
Remote storage is optional, through GitLab ciphertext sync or the Cloudflare D1 ciphertext backend
selected with `KYVAULT_BACKEND=d1`. Compared with a self-hosted secret manager, Kyvault's local mode
requires no server deployment; remote features require the corresponding GitLab repository or D1 configuration.

---

## 🌟 Like it? Show your support!

If Kyvault helps you, drop a **Star ⭐️** on the GitHub repo — every star is the biggest motivation to keep improving!

👉 **[GitHub repo](https://github.com/webkubor/kyvault)**

---

<p align="center">
  Built with 🔐 by <a href="https://github.com/webkubor">webkubor</a>
</p>