# <img src="desktop/ui/logo.png" width="36" height="36" alt=""> Kyvault

Kyvault is a local-first secret manager for developers. Ky for Mac keeps passwords, quick notes and images on your Mac. The CLI encrypts API keys and tokens with AES-256-GCM and lets tools such as Codex and Claude Code reference credentials by alias.

[![CLI tests](https://github.com/webkubor/kyvault/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/webkubor/kyvault/actions/workflows/test.yml) [![Desktop checks](https://github.com/webkubor/kyvault/actions/workflows/desktop.yml/badge.svg?branch=main)](https://github.com/webkubor/kyvault/actions/workflows/desktop.yml) [![CLI release](https://img.shields.io/github/v/release/webkubor/kyvault?filter=v*&label=CLI)](https://github.com/webkubor/kyvault/releases/latest) [![MIT license](https://img.shields.io/github/license/webkubor/kyvault)](LICENSE)

**[Download Ky 0.3.5 for Mac (Apple Silicon)](https://github.com/webkubor/kyvault/releases/download/desktop-v0.3.5/Ky_0.3.5_aarch64.dmg)** · [CLI downloads for macOS, Linux and Windows](https://github.com/webkubor/kyvault/releases/tag/v2.8.3) · [Website](https://webkubor.github.io/kyvault/) · [中文](README.md)

[![Ky 0.3.5 desktop home screen](docs/assets/desktop/home-motion.png)](docs/assets/desktop/home-motion.mp4)

https://github.com/user-attachments/assets/e23302a4-63e8-4180-a989-88c326ff67f4

## On your Mac

- **Capture a thought.** Press ⌥⌘N to write and ⌘↩ to save privately. You can also collect selected text from a webpage. [Browser setup](desktop/browser/README.md)
- **Find a credential.** Search by platform or purpose, then open the result to reveal its value.
- **Share an idea.** Select paragraphs and images from a draft and copy the card into an editor that accepts images.

Sync your password vault to a private GitHub or GitLab repository. The master key and private notes stay on your Mac. [Desktop guide](desktop/README.md)

## From the terminal

With Rust installed, install the CLI and list its commands.

```sh
cargo install kyvault
kyvault --help
```

See the [CLI guide](docs/cli-guide.en.md) for other installation options, commands and backups, or [configure your agent](docs/agent-guide.md) to use credential aliases.

[Changelog](CHANGELOG.md) · [Security](SECURITY.md) · [Contributing](CONTRIBUTING.md) · [License](LICENSE)
