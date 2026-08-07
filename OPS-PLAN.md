# Keyring 运营计划

## 发布时间线

### Day -3: 准备
- [x] README 金字塔重写
- [x] 测试覆盖（36个）
- [x] CI/CD 配置
- [ ] 配图/logo（用现成服务生成）

### Day 0: 首发（周二上午 10:00 UTC+8）
- [ ] 推送 v1.0.0
- [ ] 发 Hacker News (Show HN)
- [ ] 发 Twitter/X
- [ ] 发 Reddit r/programming

### Day 1-3: 传播
- [ ] 发掘金/少数派
- [ ] 发 V2EX
- [ ] 回复所有评论

### Day 7: 复盘
- [ ] 分析 star 来源
- [ ] 收集反馈

---

## 首发帖模板

### Hacker News (Show HN)

```
Show HN: Keyring – AI-safe secret management for developers

I built Keyring to solve a problem I kept running into: AI coding assistants (Copilot, Cursor, Claude) can read .env files and environment variables, exposing secrets in logs and context.

Keyring stores secrets locally with AES-256-GCM encryption. You set aliases, and AI agents only see the alias names – never the actual values.

Key features:
- AI-safe alias injection (agents see `github_token`, not `ghp_xxx`)
- Pure local storage, zero network dependency
- .env file migration with one command
- Works with any AI assistant (Claude, Copilot, Cursor)

GitHub: https://github.com/webkubor/agent-secret-skills

Would love your feedback!
```

### Twitter/X

```
🔐 Keyring — AI时代密钥管理

你的 AI 助手能读 .env 文件吗？能读环境变量吗？

这意味着你的 token 正在暴露。

Keyring 解决这个问题：
✅ 本地加密存储（AES-256-GCM）
✅ AI 只看到别名，看不到明文
✅ 30秒上手

开源 + MIT: github.com/webkubor/agent-secret-skills

#AI #Security #OpenSource
```

### Reddit r/programming

```
Title: Keyring - AI-safe secret management (your AI can't read your tokens)

Body:
Hey r/programming,

I've been using AI coding assistants (Copilot, Cursor, Claude) and realized they can read .env files and environment variables. That means any secret in your project is visible to the AI.

I built Keyring to fix this. It stores secrets locally with AES-256-GCM encryption. You create aliases, and when AI needs a token, it only sees the alias name.

Quick demo:
```
# Store your token
keyring set secret://github/my-pat "ghp_xxx"

# Create alias
keyring alias set github_token secret://github/my-pat

# AI runs this - sees "github_token", not the actual token
keyring run --env GITHUB_TOKEN=github_token -- git push
```

Features:
- AES-256-GCM encryption
- Pure local, no network
- .env import
- Works with any AI assistant

GitHub: https://github.com/webkubor/agent-secret-skills

Feedback welcome!
```

### 掘金/少数派

```
标题：Keyring：AI 时代密钥管理，你的 token 还安全吗？

## 背景

用 AI 写代码越来越普遍，但有个安全隐患：AI 动手能读 .env 文件和环境变量。

你的 GitHub token、数据库密码、API key，在 AI 面前裸奔。

## 解决方案

Keyring —— 本地加密密钥管理工具。

核心思路：**人存一次，AI 用别名注入，永远看不到明文。**

## 快速上手

```bash
pip install kyvault
keyring init
keyring set secret://github/my-pat "ghp_xxx"
keyring alias set github_token secret://github/my-pat
keyring run --env GITHUB_TOKEN=github_token -- git push
```

AI 只看到 `github_token`，看不到 `ghp_xxx`。

## 核心特性

| 特性 | 说明 |
|------|------|
| 🔒 AI 安全 | 别名注入，明文不暴露 |
| ⚡ 极速 | 纯本地，毫秒级响应 |
| 📦 零依赖 | 只需 Python 3.10+ |
| 🔄 兼容 | 支持 .env 导入 |

## 为什么不用 .env？

| 方案 | AI 能读明文？ | 安全性 |
|------|--------------|--------|
| .env | ✅ 能 | ❌ 危险 |
| 环境变量 | ✅ 能 | ⚠️ 有风险 |
| **Keyring** | **❌ 不能** | **✅ 安全** |

## 总结

AI 时代，密钥管理需要新思路。Keyring 是一个轻量级解决方案。

开源地址：github.com/webkubor/agent-secret-skills
```

---

## 配图需求

用现成服务生成：

1. **Logo** (512x512)
   - 锁 + AI 元素
   - 简洁现代风格
   - 可用于 GitHub 头像

2. **Social Share** (1200x630)
   - 项目截图 + 标题
   - 用于 Twitter/HN 链接预览

3. **架构图** (1200x800)
   - 展示加密流程
   - 用户 → Keyring → 加密存储 → AI（只看到别名）

---

## 渠道优先级

| 渠道 | 预期效果 | 优先级 |
|------|----------|--------|
| Hacker News | 1000+ star | ⭐⭐⭐ |
| Twitter/X | 技术圈传播 | ⭐⭐⭐ |
| Reddit | 200+ star | ⭐⭐ |
| 掘金 | 国内开发者 | ⭐⭐ |
| V2EX | 技术讨论 | ⭐ |
| 少数派 | 效率工具 | ⭐ |
