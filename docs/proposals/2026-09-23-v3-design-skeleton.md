---
audience: [human, agent]
tags: [kyvault, design, kdf, recovery, integrity, gitlab-backend]
priority: high
status: ready-for-review
date: 2026-09-23
covers:
  - webkubor/kyvault#5 (master.key 派生)
  - webkubor/kyvault#1 (PEM stdin 截断)
  - webkubor/kyvault#2 (PEM 完整性)
  - webkubor/kyvault#3 (GitLab 私仓后端)
---

# kyvault v3.0 — Master Key 派生 + 完整性校验 + GitLab 后端

> 把 4 个开放 issue 收敛到一次设计升级。
> 现状（v2.3）已是「加密学正确 + 部署错位」，v3.0 把部署摆正，让 agent 服务器能自助解锁。

---

## TL;DR（决策三连）

1. **Master.key 派生**：Argon2id + 双因子 —— recovery code（24 词 BIP-39）+ SSH key（ed25519/ECDSA）。Owner 任选其一，agent 默认走 SSH key。
2. **完整性**：每个密文加 SHA-256 + byte_len + line_count metadata，get 时校验；任何不匹配都 fail-loud。
3. **存储后端**：GitLab 私仓是唯一真源，D1 路径**删除**（不只 disable）。

详细论证 + 分阶段路线见下。

---

## 1. Master.key 派生（覆盖 #5）

### 现状问题

```
~/.keyring/
├── master.key       # 裸 base64 文件，chmod 600
└── secrets.json     # AES-256-GCM 密文
```

协作流程：owner 在 Mac 上 init → push 密文到 GitLab → 协作者 clone 拿到密文，**但 master.key 必须 owner 人肉带外**（AirDrop / U盘 / scp）。

agent 场景（vex 在 43.157.251.114 上跑）：
- ✅ clone GitLab 仓 → 拿到密文
- ❌ 没有 master.key → 解不开任何东西

### 方案对比

| 方案 | owner 体验 | agent 体验 | 安全 |
|---|---|---|---|
| **A. Passphrase + Argon2id** | 每次解锁输密码 | owner 必须告知密码 | 密码强度依赖 owner |
| **B. Recovery code（BIP-39 24 词）+ SSH key 双因子** | init 一次性输出 24 词，**纸上/密码管理器备份**；之后 SSH key 自动解锁 | 服务器上有自己的 SSH key → 自动解锁 | 24 词 = 256 bit 熵；SSH key = 现成的非对称凭证 |
| **C. 只用 SSH key** | init 时绑 owner 的 SSH pub key | 任何机器上只要有匹配的 SSH priv key 就能解锁 | 单点：SSH key 一旦泄露，全完 |

**推荐 B**：24 词做最后兜底（owner 失 SSH key 时仍能恢复），日常自动走 SSH key。

### 具体设计

```
~/.keyring/
├── master.key.enc       # v2.3 的 master.key 用 passphrase/24词加密后的密文
├── ssh-binding.json     # {"ssh_key_fp": "SHA256:abc...", "algorithm": "argon2id"}
├── unlock-factor.json   # 标记当前默认解锁方式（ssh | recovery | passphrase）
└── secrets.json         # 密文（不变）
```

**初始化**（`kyvault init`）：

```bash
# 1. 生成 master.key (256 bit 随机)
# 2. 输出 24 词 recovery code（一次性，paper / password manager 保存）
# 3. 检测 ~/.ssh/id_ed25519.pub，自动写入 ssh-binding.json
# 4. 用 recovery code 派生一个 key wrap master.key → master.key.enc
```

**解锁**（`kyvault get` / `kyvault run`）：

```bash
# 优先级
1. 若有匹配的 SSH key（指纹匹配 ssh-binding.json）→ 自动解锁
2. 否则读 $KYVAULT_RECOVERY_CODE 环境变量（agent 服务器：secret://kyvault/recovery-code-from-owner）
3. 否则 stdin 提示输入 24 词（owner 交互模式）
```

**迁移路径**（v2.3 → v3.0）：
- `kyvault migrate --add-recovery` → 给现有 master.key 加 recovery code + SSH binding
- 不丢任何已有密文，向后兼容

---

## 2. 多行 / PEM 完整性（覆盖 #1 + #2）

### 现状问题

存 PEM 时：
- stdin `-` 路径只读首行 → 26 字节只剩 `-----BEGIN PUBLIC KEY-----`
- `--value @file` 不被支持 → 存的是字面字符串
- 没有 SHA-256 校验 → 截断 / 转义问题发现不了

### 设计：密文 metadata

```json
{
  "secret://wechatpay/pub-key-pem": {
    "ciphertext": "base64...",
    "nonce": "base64...",
    "metadata": {
      "sha256": "abc123...",
      "byte_len": 1704,
      "line_count": 27,
      "kind": "PEM",
      "created_at": "2026-09-19T..."
    }
  }
}
```

**set 时自动计算**：
- `sha256` = SHA-256(明文)
- `byte_len` = 明文字节数
- `line_count` = 明文行数（多行 token / PEM 判定）

**get 时校验**：
- 解密 → 重新算 sha256 → 比对 metadata.sha256
- 不匹配 → 报错 + warn + 不返回明文（fail-loud）

**新增入口**：
- `--file @/path/to/file`（替代 stdin `-`）：从文件读明文
- `--multiline` flag 显式声明：允许 stdin 多行
- 不再有"读一行"的隐式行为

**新增 lint**：
- `kyvault lint <alias>` — 检查所有 PEM/JSON 密钥的完整性，输出损坏清单
- CI 里跑一遍，早发现问题

---

## 3. GitLab 私仓后端（覆盖 #3）

### 现状问题

`KYVAULT_BACKEND=file|d1`（默认 file）。D1 路径让 cs 必须持有 Cloudflare token，**调度系统被迫持有密钥** —— 错误分层。

### 设计：删 D1，不只是 disable

```rust
// 删除 KYVAULT_BACKEND env var
// 移除 rust/src/d1.rs 整个文件
// 仅保留 GitLab backend
```

**GitLab backend 协议**：

```
secrets.json (密文 + metadata)  →  GitLab private repo webkubor/kyvault-store
~/.ssh/config: Host gitlab.com → 自动用 owner 的 SSH key
```

**命令**：
- `kyvault gitlab push` → 提交所有变更（自动 commit + push）
- `kyvault gitlab pull` → 拉取最新密文 + 触发解锁（用 SSH key）
- `kyvault gitlab status` → 显示本地 vs 远程的差异

**协作模型**：
- 每个协作者把自己的 SSH pub key 加进 GitLab project deploy keys
- 任何人 push 的密文，其他人都能用自己 SSH key 解锁（因为 master.key 是同一份，绑定到 SSH key pub fingerprint）
- 紧急撤销：删 deploy key + `kyvault rotate-master` 重新初始化

### 删除 D1 的成本

`rust/src/d1.rs` 头注释已经写明「连 D1 的 token 本身就是密钥」，删它是减负。**保留迁移路径**：

```bash
# 从 D1 导出所有密钥到本地 file backend，再 init GitLab backend
kyvault migrate --from d1 --to gitlab --output ./migration-secrets.json
```

---

## 4. 分阶段路线（建议 PR 顺序）

| Phase | 涉及 | 内容 | 风险 |
|---|---|---|---|
| **v2.4.0** | 完整性 | 加密文 metadata（sha256/byte_len/line_count）；新增 `--file @path` 入口；`kyvault lint` | 低，纯增量 |
| **v2.5.0** | stdin 修复 | 修 `cs kyvault set` 的 ReadString('\n') bug；显式 `--multiline` flag | 低 |
| **v3.0.0** | 派生 + 后端 | Argon2id + SSH binding + recovery code；删 D1 backend；迁移路径 | 中（破坏性升级） |

每个 phase 独立可发版，**不要合并到一个大 PR** —— 单 PR review 不可能覆盖三件事。

---

## 5. 与 cs（CortexOS）的边界

- `cs kyvault` 这一层**继续保留**作为 wrapper（agent 不直接调 kyvault 二进制）
- 但 `cs kyvault set` 必须修：`bufio.Reader.ReadString('\n')` 改成读全部 + 支持 `@file` + 显式 `--multiline`
- **D1 依赖是 cs 的，不是 kyvault 的** —— cs 的 D1 token 还在用，**kyvault 自己退 D1 不影响 cs 其他功能**

---

## 6. 待 owner 拍板

- [ ] 派生方案选 A / **B（推荐）** / C
- [ ] v3.0 是否需要向后兼容 v2.x 密文格式？（建议：自动检测 + 升级，密文 v3 标记）
- [ ] `kyvault-store` GitLab repo 的可见性：owner-only / team-wide（含协作者）？
- [ ] 24 词 recovery code 强制 paper backup，还是允许写到 1Password？