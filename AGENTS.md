# AGENTS.md — AI 助手工作守则

## 这是什么

kyvault —— AI 时代的开发者密钥与资产台账管理器（Rust）：AES-256-GCM 加密存本机，
工具凭**别名**取用，agent 不接触明文。核心承诺：**减少明文密钥进入模型上下文**。

## 关键文件

- `rust/`：主程序源码（`rust/src`）与测试（`rust/tests`）。
- `homebrew/`：Homebrew tap 配方；`packaging/`：各平台打包。
- `SKILL.md`：agent skill 定义 —— agent 侧的用法口径以它为准。
- `SECURITY.md`：安全上报通道；**密钥与加密相关改动前必读**。
- `docs/`：设计文档与提案（outreach / proposals）。

## 常用命令

```bash
cd rust && cargo build    # 构建
cd rust && cargo test     # 测试
```

## 约定（安全红线）

- **任何改动不得把明文密钥打进日志、错误信息或 stdout**；解密后的值只在注入目标进程时短暂存在。
- 加密实现（算法、nonce 生成、KDF）的任何改动都必须带测试与迁移说明，不允许静默换格式。
- 本仓库自己不用密钥做 CI 测试 —— 测试一律用 fixture 假值。
