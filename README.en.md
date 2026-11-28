<h1 align="center"><img src="desktop/ui/logo.png" width="36" height="36" alt=""> Kyvault</h1>

<p align="center"><strong>A local password vault and private notebook for Mac, with a secret manager CLI for developers. No Ky account required.</strong></p>

<p align="center">
  <a href="https://github.com/webkubor/kyvault/actions/workflows/test.yml"><img src="https://github.com/webkubor/kyvault/actions/workflows/test.yml/badge.svg?branch=main" alt="CLI tests"></a>
  <a href="https://github.com/webkubor/kyvault/actions/workflows/desktop.yml"><img src="https://github.com/webkubor/kyvault/actions/workflows/desktop.yml/badge.svg?branch=main" alt="Desktop checks"></a>
  <a href="https://github.com/webkubor/kyvault/releases/latest"><img src="https://img.shields.io/github/v/release/webkubor/kyvault?filter=v*&amp;label=CLI" alt="CLI release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/webkubor/kyvault" alt="MIT license"></a>
</p>

| Choose your tool | What you can do | Download |
| --- | --- | --- |
| **Ky for Mac** | Find a password, capture a thought with ⌥⌘N, or share selected text and images as a card. | [Mac 0.3.5 · Apple Silicon (arm64)](https://github.com/webkubor/kyvault/releases/download/desktop-v0.3.5/Ky_0.3.5_aarch64.dmg) |
| **Kyvault CLI** | Pass an API key or token to a child process by alias. Your command contains only the alias. | [CLI 2.8.3 · macOS, Linux, Windows](https://github.com/webkubor/kyvault/releases/tag/v2.8.3) |

[Website](https://webkubor.github.io/kyvault/) · [中文](README.md)

[![Ky 0.3.5 desktop home screen](docs/assets/desktop/home-motion.png)](docs/assets/desktop/home-motion.mp4)

https://github.com/user-attachments/assets/e23302a4-63e8-4180-a989-88c326ff67f4

## On your Mac

- **Capture a thought.** Press ⌥⌘N to write and ⌘↩ to save privately. You can also collect selected text from a webpage. [Browser setup](desktop/browser/README.md)
- **Find a credential.** Search by platform or purpose, then open the result to reveal its value.
- **Share an idea.** Select paragraphs and images from a draft and copy the card into an editor that accepts images.

Sync your password vault to a private GitHub or GitLab repository. The master key and private notes stay on your Mac. [Desktop guide](desktop/README.md)

## From the terminal

Install with `cargo install kyvault`. If you’ve saved a key at `secret://openai/api-key`, give it an alias and run your script.

```sh
kyvault alias set openai_key secret://openai/api-key
kyvault run --env OPENAI_API_KEY=openai_key -- python app.py
```

See the [CLI guide](docs/cli-guide.en.md) for other installation options, commands and backups, or [configure your agent](docs/agent-guide.md) to use credential aliases.

[Changelog](CHANGELOG.md) · [Security](SECURITY.md) · [Contributing](CONTRIBUTING.md) · [License](LICENSE)
