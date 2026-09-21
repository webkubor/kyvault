# Proposal: GitLab 后端替代 D1（团队协作路径）

- **状态**：v2 draft — Codex v1 review 已落地 P0-1，决策点 6 待跨项目协调
- **作者**：webkubor + kyvault 维护
- **日期**：2026-09-19
- **目标版本**：2.3.0（紧随 2.2.0 之后）

## v1 → v2 修订索引

v1 文档保留在此不动，便于审计追踪。Codex v1 review 的 5 条 P0/P1 + 4 条设计漏洞
+ 6 条决策点意见全部吸收进下方「v2 修订（2026-09-19）」章节；每条带「落地状态」。

| 修订项 | 原提议 | 落地状态 |
|---|---|---|
| `load()` 冲突工作区被覆盖 | 宽容地当空库 | ✅ 已落地（`store.rs`） |
| vault 级跨进程锁 | 仅 git 协议判据 | ⏳ 计划 v2.3 与 GitLab 后端一起 |
| D1 三件套边界（个人 / 团队库） | 未明确 | ⏳ 计划 v2.3，区分 `~/.keyring`（personal）+ `~/.keyring-team`（team） |
| 团队初始化两条流程 | 一句话带过 | ⏳ 计划 v2.3，`gitlab setup` 区分「首次建库 / 加入已有」 |
| 一次性迁移脚本 | 留给用户 | ⏳ 计划 v2.4（迁移 D1 → file 团队库） |
| visibility / 越权 404 退役 | 直接退役 | ⏸️ 待 CortexOS 维护者对齐（决策点 6） |
| "git log 自带审计"承诺 | 自带 | 🔧 收窄：仅 push 后的提交历史 |
| GitLab 仓库存"加密密文" | 全部加密 | 🔧 收窄：secret 值加密，名称 / 结构 / 部分元数据可见 |
| 时间表一致性 | doctor 弃用分散到 v2.3 / v2.4 | 🔧 统一到 v2.4 |

完整 review 报告：[`2026-09-19-gitlab-backend-replaces-d1.REVIEW.md`](./2026-09-19-gitlab-backend-replaces-d1.REVIEW.md)

## v2 修订（2026-09-19，吸收 Codex review）

### P0-1：冲突工作区被覆盖 —— **已落地**

Codex 抓到的核心 bug：`store.rs` 旧 `load()` 把"读不出来"和"JSON 解析失败"都宽容地
当作空库，代价是 rebase 冲突标记（`<<<<<<<` 等）进入 `secrets.json` 时，下一次
`set` 拿空对象覆盖整库。**这就是 GitLab 后端会把这个偶发 bug 变成常见 bug 的根因。**

**修订（已提交）：**

- `store.rs::load()` 签名改为 `Result<Value>`，分类处理：
  - 文件不存在 → 空对象（init 流程需要，**唯一合法空库**）
  - 读不出（IO）→ 错误"先确认文件权限，未恢复前不要写入"
  - 含 git 冲突标记 → 错误"先 git rebase --abort，未解决冲突前不要写入"
  - JSON 损坏 → 错误"不是合法 JSON"
- 全部 18 个 `self.load()` 调用点改成 `?`
- 5 个返回 `Value` / `Vec<_>` / `BTreeMap` 的 list 类方法（`list_secrets` / `list_servers` / `list_clis` / `list_bucket` / `platforms`）签名改成 `Result`，外部调用点（`main.rs`、`doctor.rs`）同步适配
- 加 helper `has_git_conflict_markers` 识别 `<<<<<<<` / `=======` / `>>>>>>>` / `|||||||` 四个标记位

**新增 4 个测试 + 修改 1 个旧测试：**

- `broken_json_blocks_writes` —— 替代原 `broken_json_does_not_panic`（旧测试验收"宽容当空库"，新行为是拒绝写入）
- `git_conflict_markers_block_writes` —— 核心场景，断言冲突文件不被覆盖
- `missing_file_still_works_for_init` —— 保护 init 流程（文件不存在 → 空库 + set 成功）
- `conflict_marker_detection` —— 单测 `has_git_conflict_markers` 自身

**验证：**

- `cargo test --lib`：40/40 通过
- CLI 烟雾测试：注入冲突标记后 `set` / `list` 都退出码 1，错误消息清晰
- 真实 `~/.keyring`（6 条记录）：`doctor` + `list` 完全兼容

### P0-2：visibility / 越权 404 退役 —— **待 CortexOS 协调**

Codex 抓到的事实：kyvault `d1.rs:180` 直接按 ID 查询密文，**没有 identity / visibility
判据**；`SKILL.md` 承诺的"越权 404、远程禁写、本机放行"实际在 CortexOS 服务端做的，
不在 kyvault。删除 Rust D1 模块**不等于**退役授权边界。

**修订：**

- v2.6 删除 D1 后端代码的硬时间表取消
- 改成"v2.6 = 最早候选移除版本"，**删除门槛**：
  1. CortexOS 维护者确认 GitLab 后端下还能不能保持 visibility
  2. 如不能，CortexOS 在 GitLab 共享密钥库上跑出可演示的"应用层 ACL"
  3. CS 大脑在 GitLab 后端下跑通完整回归测试
  4. 准备回滚方案（万一 GitLab 出问题立刻切回 D1）

### P0-3：vault 级跨进程锁 —— **计划 v2.3**

Codex 抓到：`store.rs::set_secret` 是无锁整文件读改写；`save()` 用固定
`secrets.json.tmp`。同一台机器两个 agent 同时改不同 secret，后写者覆盖先写者，
git 协议 fast-forward 也不会发现（数据丢在 commit 之前）。

**修订（GitLab 后端实施时一并做）：**

- 引入 `flock(2)` 包裹"读 + 修改 + 保存"的整段流程（`~/.keyring/secrets.json.lock`）
- 锁覆盖：所有 set / delete / annotate / gitlab push / gitlab pull
- 锁失败返回明确错误（不要静默重试）
- 这是 **store.rs 的真 bug**，跟 GitLab 后端解耦。优先级：可考虑先做 P0-3，
  再做 GitLab 后端（独立 PR）

### 决策点 1（master.key 不入仓）补充

- **保留带外分发**（管理员生成 + 成员导入同源 key）
- 加 `kyvault import-key` 命令：成员贴入 master key，自动 verify 能解密一条
  已知密文后才接受；verify 失败拒绝写入
- 文档化"成员退出轮换"：删 GitLab 成员 ≠ 收回已下载的密文和 key，必须走 master.key
  轮换流程（重新加密全库 + 重发 key）

### 决策点 2（Backend 枚举 GitLab 与 File / D1 并列）补充

- 保留当前枚举形态
- 加约束：**活动文件单一来源**。`_remote/secrets.json` 是实际 Store 文件（不是副本）
- 冲突处理：`gitlab pull` 时把远端拉到 staging 区，验证 JSON / 验证非冲突
  标记后**原子替换**当前 `secrets.json`。失败时不动当前文件

### 决策点 4（并发 fast-forward / rebase）补充

- **不再提供 `--force`**（Codex 修正了原方案第 109 行的提议）
- 提供 `kyvault gitlab pull --abort`：放弃未解决的合并，回到 pull 前状态
- 提供 `kyvault gitlab status`：明确显示"本地有未 push / 远端有未 pull / 冲突标记残留"
- 同一 clone 的并发走 vault 级锁（见 P0-3）；跨 clone 走 git 协议

### 决策点 5（一次性迁移脚本）补充

`kyvault migrate d1-to-file --target-dir <path>`：

- 源：本地默认 file 库 + D1 三件套（从 `secret://kyvault/d1-*` 拿）
- 目标：指定目录的新 file 团队库
- 流程：内存中解密 D1 密文 → 按目标 file 的 AES key 派生（注意：D1 key 直接
  base64_decode 不派生，file key 要 SHA256(base64_decode)，必须重加密）→
  按目标格式（joined base64）写出
- 元数据承接：D1 的 `kind/account/org/scopes/visibility` 不能完整迁 —— 报告列出
  未承接字段，让用户决定
- **不做长期双写，禁止"边迁边写"**：迁移期间冻结源，迁移完成切换

### 决策点 6（D1 退役与 CortexOS 对齐）补充

- v2.6 删除 D1 **不是 kyvault 单方面的事** —— 涉及 Rust CLI 适配器、
  CortexOS Go D1 客户端、数据库表、服务端权限层四块
- 待发送一份对齐 message 给 CortexOS 维护者（见 todo C）
- 在对齐回复回来之前，**不做**任何 D1 删除 / 退役文档的硬编码

### 设计漏洞 1：D1 三件套会泄漏到团队库

`main.rs:403-409` 把 D1 三件套存在默认 `~/.keyring` 库的 `secret://kyvault/d1-*`。
GitLab 后端 sync 整个库 = 把 CF token 交给团队所有成员。

**修订：**

- 引入 `personal vault` vs `team vault` 概念
  - `~/.keyring/`（默认）= personal vault，不参与 sync
  - `~/.keyring-team/`（新路径）= team vault，由 GitLab 后端管
- `KYVAULT_BACKEND=gitlab` 默认指向 `~/.keyring-team/`
- `secret://kyvault/d1-*` 显式落在 personal vault，D1 后端退役后整体清理

### 设计漏洞 2 / 3：承诺收窄

原方案第 126 行 "git log 自带审计" 承诺过强；原方案第 49、126 行 "加密密文入仓" 不准。

**修订：**

- "git log 自带审计" → 仅指 push 后的提交历史（每条 push 是一个 commit，diff 可读）。
  本地 `get` / `run` 不形成读取审计；显式 push 前的多次本地修改未必逐次留痕（只在
  working tree 状态 + 最终 push diff 体现）
- "加密密文入仓" → "secret 值加密，名称 / 结构 / 部分元数据可见"。JSON 顶层键
  （platform、name）明文，密文是 base64 字符串，名字可读 = 团队成员能看到
  "secret://github/pat 存在" 但看不到 pat 值

### 设计漏洞 4：时间表统一

原方案 v2.3 / v2.4 都写"doctor 弃用警告"，分散。修订：

| 版本 | 内容 |
|---|---|
| v2.3 | GitLab 后端实现；D1 代码保留不删；doctor 加"推荐 GitLab"提示（不警告） |
| v2.4 | 一次性迁移脚本发布；doctor 加"D1 后端 deprecation" 警告 |
| v2.5 | D1 后端编译期 deprecation lint（`#[deprecated]`） |
| v2.6 | **最早候选移除版本**，删除门槛见决策点 6 |

## 1. 背景

kyvault 当前（v2.2.0）有两条后端路径：

| 后端 | 存储 | master.key | 适用 |
|---|---|---|---|
| `file`（默认） | 本机 `~/.keyring/secrets.json` | 本机 `~/.keyring/master.key` | 单机 |
| `d1`（`KYVAULT_BACKEND=d1`） | Cloudflare D1 `secret_vault` 表 | D1 `site_config.secret_vault_master_key` | CS 大脑 / 多 agent 边界 |

两个后端 **不在同一个抽象层** —— `Store` 和 `D1` 各有自己的 `set_secret/get_secret/list_secrets`，由 `main.rs::Backend::{get,set,delete,list}` 按 `KYVAULT_BACKEND` 环境变量分发。

## 2. 问题

### 2.1 现实摩擦
- **D1 学习曲线高** —— Cloudflare 账号 / Workers 上下文 / wrangler / SQL schema，对「想要一个加密密钥管理工具」的开发者不友好
- **D1 三件套冗余** —— 连 D1 的 CF token 本身就是密钥，被 kyvault 加密存在本地 file 库里做自举（`secret://kyvault/d1-*`），依赖链条绕一圈
- **file / D1 两套数据不通** —— CS 大脑用 D1，本机 `~/.keyring` 只能看到 file 库的 6 条记录；两边不能互看，造成"我存过了怎么找不到"的真实事故

### 2.2 团队协作路径缺失
- kyvault 是公开开源项目（README 标注 MIT），目标是给"开发者"管理密钥
- 当前没有"团队共享一个密钥库"的开箱方案：要么每台机器自维护 file（不能协作），要么都迁到 D1（要 CF 知识）
- 团队用户实际期望："git pull 就看见队友改了啥"——这是 GitLab/GitHub 私有仓的心智模型

### 2.3 多 agent 边界不是主流场景
- D1 的 `visibility` / `org` / `scopes` / 越权 404 这些是 CortexOS（CS 大脑）的企业级能力
- 单人 / 小团队场景不需要，强行让所有人学这套边界等于抬高入门门槛
- D1 后端的特殊列（`visibility`, `org`, `scopes`）依赖 D1 SQL schema，迁不到 file 后端

## 3. 目标

1. **团队共享密钥库**有"开发者熟悉"的方案 —— git 私有仓
2. **不引入新真源** —— GitLab 后端是 file 的 git 远端镜像，不是第三个独立 schema
3. **退出 D1 依赖** —— D1 后端代码保留但废弃，新文档/推荐路径不再提
4. **保留所有现有能力** —— 22 个命令行为不变；D1 用户路径不被立即打破

## 4. 提议方案

### 4.1 新增 GitLab 后端（角色：**file 的 git 远端镜像**）

```
master.key   → 本机 ~/.keyring/master.key，绝不入仓（这是安全前提）
secrets.json → 加密内容 push 到 gitlab.com/<user>/<vault-repo>
团队成员：
  1. git clone 仓库到 ~/.keyring/_remote/  （或自定义路径）
  2. kyvault init 自己生成 master.key 副本（与团队共享的同源 —— 见 4.3）
  3. kyvault gitlab pull 从远端拉密文
  4. 本地 set / delete 走本地 store，kyvault gitlab push 同步到远端
```

### 4.2 后端分发扩展

`main.rs::Backend` 改为：

```rust
enum Backend {
    File(Store),
    D1(D1),      // 保留但 deprecated
    GitLab { local: Store, remote: GitLabRemote },
}
```

`KYVAULT_BACKEND` 三态：`file`（默认）/ `gitlab` / `d1`（deprecated，警告）。

### 4.3 master.key 共享策略

**核心约束**：master.key 不入仓。

团队成员共享 master.key 通过**带外信道**：

| 方案 | 适合 |
|---|---|
| 仓库管理员用 1Password / Bitwarden 团队 vault 共享 | 已用 1Password 的小团队 |
| 仓库管理员用飞书加密消息把 key 文件发给成员 | 已经用飞书的 |
| 内部 KMS 派生 master key | 中大规模团队 |

不在 kyvault CLI 里实现 master.key 共享 —— 那是个信道问题，不是 kyvault 的责任。

### 4.4 命令面新增（GitLab 子命令）

```
kyvault gitlab setup <repo-url>   # 克隆远端仓库 + 初始化本地（要 master.key）
kyvault gitlab push               # 把本地 secrets.json 加密内容 commit + push
kyvault gitlab pull               # 拉远端最新（要 rebase 模式，避免覆盖本地）
kyvault gitlab sync               # pull → 冲突检测 → push
kyvault gitlab status             # 落后几次提交 / 是否有本地未同步
```

### 4.5 D1 后端处理

- **代码保留**：`rust/src/d1.rs` 不删，CS 大脑当前生产路径不能立即断
- **文档标注 deprecated**：SKILL.md / README.md / doctor 输出加「D1 维护模式，新部署请走 GitLab」提示
- **doctor 警告**：检测到 `KYVAULT_BACKEND=d1` 时输出「D1 后端已弃用，建议迁移到 GitLab 后端」
- **删除时间表**：3 个 minor 版本后（即 v2.6.0 移除）。d1-setup 命令也保留到那时

### 4.6 并发与冲突

| 场景 | 处理 |
|---|---|
| pull 时远端领先本地 | fast-forward 合并，正常 |
| push 时远端领先本地 | **拒绝 push**，要求先 pull |
| 本地有未 push 改动 + 远端有更新 | 自动 rebase；冲突（同一 secret id 都被改）报错要求手动 |
| 强 push (`--force`) | 显式 `--force` 才允许，默认拒绝

## 5. 不做什么（Out of Scope）

1. **不把 D1 密文镜像到 GitLab** —— 两条路径独立，互不并网。用户选后端，自己负责迁移
2. **不加 visibility 到 GitLab 后端** —— GitLab 仓库存密文，clone 的人拿到全集；强行做"按身份过滤"等于把 ACL 嵌进 git 协议，不是 kyvault 该做的
3. **不在 GitLab 后端实现 master.key 共享** —— 见 4.3
4. **不改加密格式** —— `rust/src/crypto.rs` 不动，GitLab 后端复用同一套 AES-256-GCM
5. **不破坏现有 22 个命令** —— `set`/`delete`/`list`/`get` 行为在 GitLab 后端下跟 file 后端完全一致（push 是可选的显式动作）

## 6. 风险

| 风险 | 缓解 |
|---|---|
| master.key 通过带外信道泄漏 | 文档强调"master.key = 全部密钥"，跟 GitHub PAT 同级管理 |
| GitLab 服务故障，团队读不到最新 | 本地 file 仍可读，看不到的是队友改动 |
| 两个 agent 同时改同一行 secret | pull 拒绝非 fast-forward + rebase 冲突检测（见 4.6） |
| GitLab 仓库存密文 ≠ 加密硬盘，密文永远在 GitLab 上 | 这是设计取舍：换来"开发者熟悉的心智模型 + 自带审计 git log"。接受这条代价 |
| D1 后端退役时 CS 大脑生产路径断 | 3 个 minor 周期（v2.3 → v2.6），中间默认 D1 + GitLab 都能跑 |

## 7. 迁移路径

| 阶段 | 内容 |
|---|---|
| v2.3.0 | 实现 GitLab 后端，D1 保留，新文档主推 GitLab |
| v2.4.0 | doctor 警告 D1 弃用，README 默认示例改 GitLab |
| v2.5.0 | D1 后端加 deprecation lint（编译期 warn） |
| v2.6.0 | 移除 D1 后端代码（rust/src/d1.rs 删除、main.rs::Backend::D1 变体删除、`d1-setup` 命令删除） |

## 8. 关键决策点（请 Codex 重点 review）

1. **master.key 不入仓** —— 这是安全前提，但意味着团队必须有"带外共享 master.key"的流程。这是技术债还是合理责任划分？
2. **GitLab 后端不是独立后端，是 file 的远端镜像** —— 但 `main.rs::Backend` 枚举里跟 `File`/`D1` 并列，看起来像是三个独立后端。这种"形态上并列、逻辑上同一个源"的设计会不会误导？
4. **D1 后端的"visibility / 越权 404"能力退役** —— 等于关掉 CS 大脑的多 agent 边界层。v2.6 之前 CS 大脑怎么过渡？是把 visibility 落到应用层（CortexOS 自己做）还是接受这段窗口期无 visibility？
5. **并发冲突用 git 协议判据（fast-forward / rebase）** —— 这是 git 的能力边界，不是 kyvault 加的。但 kyvault 加了 push 前 pull 这种逻辑后，git 协议还能否撑住"两个 agent 同时改不同 secret"的常见场景？
6. **不并网 D1 和 GitLab** —— 用户如果两边都有数据，迁不自动两边获。具体后果：CS 大脑当前 GitLab 上没仓库，要新建；现有的 D1 密文如果想迁到 GitLab 后端，要重新走 `set`。这是不是合理的代价？

## 9. 参考

- `rust/src/store.rs`（file 后端，727 行）—— GitLab 后端会复用其加密逻辑
- `rust/src/d1.rs`（449 行）—— 即将废弃的 D1 后端实现
- `rust/src/main.rs:207-259`（Backend 分发点）—— 新增 `GitLab` 变体要改的地方
- `rust/src/lib.rs`（模块导出）—— 加 `pub mod gitlab;`
- `SKILL.md:232-277`（多 agent 环境文档）—— v2.6 退役时可删整段