<h1 align="center"><img src="desktop/ui/logo.png" width="36" height="36" alt=""> Kyvault</h1>

<p align="center"><strong>Mac 上的本地密码库与私密闪记，也是开发者的命令行密钥管理工具。无需注册 Ky 账号。</strong></p>

<p align="center">
  <a href="https://github.com/webkubor/kyvault/actions/workflows/test.yml"><img src="https://github.com/webkubor/kyvault/actions/workflows/test.yml/badge.svg?branch=main" alt="CLI tests"></a>
  <a href="https://github.com/webkubor/kyvault/actions/workflows/desktop.yml"><img src="https://github.com/webkubor/kyvault/actions/workflows/desktop.yml/badge.svg?branch=main" alt="Desktop checks"></a>
  <a href="https://github.com/webkubor/kyvault/releases/latest"><img src="https://img.shields.io/github/v/release/webkubor/kyvault?filter=v*&amp;label=CLI" alt="CLI release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/webkubor/kyvault" alt="MIT license"></a>
</p>

| 选哪个 | 可以做什么 | 下载 |
| --- | --- | --- |
| **Ky 桌面版** | 按平台查密码，按 ⌥⌘N 记下想法，把选中的图文复制为分享卡片。 | [Mac 0.3.5 · Apple Silicon（arm64）](https://github.com/webkubor/kyvault/releases/download/desktop-v0.3.5/Ky_0.3.5_aarch64.dmg) |
| **Kyvault 命令行** | 通过别名把 API Key 或 Token 传给子进程，命令里只写别名。 | [CLI 2.8.3 · macOS、Linux、Windows](https://github.com/webkubor/kyvault/releases/tag/v2.8.3) |

[官网](https://webkubor.github.io/kyvault/) · [English](README.en.md)

[![Ky 0.3.5 桌面版首页实拍](docs/assets/desktop/home-motion.png)](docs/assets/desktop/home-motion.mp4)

https://github.com/user-attachments/assets/e23302a4-63e8-4180-a989-88c326ff67f4

## 在 Mac 上开始

- **记想法**　按 ⌥⌘N 直接写，⌘↩ 存到「私密收藏」。读网页时也可以选中文字，右键收进闪记。[按网站自动收藏](desktop/browser/README.md)
- **查密码**　在「我的密钥」搜平台和用途，例如「支付宝 私钥」。点开凭证就能查看原值。
- **分享图文**　选好草稿中的段落和配图，点「复制卡片」即可粘贴到支持图片的编辑器。

密码库可以同步到自己的 GitHub 或 GitLab 私有仓库。主密钥和私密图文留在本机。[桌面版使用说明](desktop/README.md)

## 在终端使用

运行 `cargo install kyvault` 安装命令行工具。假设已保存密钥 `secret://openai/api-key`，给它起个别名，再运行你的脚本。

```sh
kyvault alias set openai_key secret://openai/api-key
kyvault run --env OPENAI_API_KEY=openai_key -- python app.py
```

其他安装方式、命令示例和备份步骤见 [CLI 使用手册](docs/cli-guide.md)。要让 Codex 或 Claude Code 使用密钥别名，见 [Agent 配置](docs/agent-guide.md)。

[更新日志](CHANGELOG.md) · [安全说明](SECURITY.md) · [参与贡献](CONTRIBUTING.md) · [许可证](LICENSE)
