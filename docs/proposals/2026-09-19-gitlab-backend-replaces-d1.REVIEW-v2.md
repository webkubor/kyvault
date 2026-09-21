# Review v2: GitLab 后端替代 D1 方案

- **对象**：`docs/proposals/2026-09-19-gitlab-backend-replaces-d1.md`（v2 修订版）
- **Reviewer**：Codex（`codex exec --sandbox read-only`，独立 child session，67k tokens）
- **日期**：2026-09-19
- **方式**：静态审查；读了 v1 review、v2 修订、`store.rs`/`main.rs`/`doctor.rs` 的完整 diff；额外跑了 `cs brief / cs flow / cs rule security_boundary / cs rule coding_conventions` 把 kyvault 放回 CortexOS 治理框架

## Verdict

**revise**

v2 实质吸收了错误传播、禁止强推、迁移重加密和条件式退役，但 P0-1 修复引入新回归
（冲突误判合法数据），权限承接、同步事务和团队初始化仍缺关键约束。

## P0-1 修复确认

- **四类输入的主分支处理正确：**
  - 文件不存在（NotFound）→ 空对象
  - 真正的 Git 冲突标记 → 错误，不进入保存
  - JSON 语法损坏 → 解析错误向上传播
  - 其他读取错误（权限、非 UTF-8）→ 错误

- **边界遗漏 / 新引入回归：**
  - **P1，新回归：全文件 `contains` 会拒绝合法数据**——名称、旧元数据及未知字段
    都允许包含 `=======` 等字符串。`set secret://a/======= v` 可以先成功写入，
    下一次读库却被冲突检测拒绝。`parse_ref()` 没有限制这种名称。
    **修复方向**：只在 JSON 解析失败后用冲突检测细化错误；或识别真实冲突行
    （行首匹配）而非全文件 contains
  - **遗留结构校验缺口**：`null` / `[]` 是合法 JSON，`list_secrets()` 返回空列表，
    `set_secret()` 在 `obj_mut()` 中可能 panic
  - **NotFound 不等于确认首次初始化**：悬空符号链接也可能返回 NotFound

四个测试的评价：

| 测试 | 结论 |
|---|---|
| `broken_json_blocks_writes` | 有效 |
| `git_conflict_markers_block_writes` | **格式错**：用了 `\<n>` 转义，实质是合法 JSON 字符串 |
| `missing_file_still_works_for_init` | 有效 |
| `conflict_marker_detection` | 覆盖部分标记，但缺少"合法 JSON 字符串不误判"用例 |

**P0-1 修复后追加（已落地，commit 后再 review）：**

- `load()` 改成先 JSON 解析，**只在失败时**细化冲突标记检测（而不是先 contains）
- 加测试 `legal_strings_with_marker_chars_remain_readable`：合法 JSON 含 `=======`
  等字符串，`list_secrets` 正常列出 keys 桶里的记录
- 修 `git_conflict_markers_block_writes`：用真实行式冲突（不在 JSON 字符串里）
- 错误消息更正："set/delete 会被拒绝"（不再是"会覆盖原数据"）
- 41/41 测试通过

## v2 修订逐条意见

### 修订项 1：`load()` 拒绝损坏库

**部分通过。** 核心数据覆盖问题已修，但冲突误判是本 PR 的阻断项。
**已修复（见上）**。

此外方案第 35 行写"已提交"，当前三个 Rust 文件仍相对 HEAD 有修改——证据只
确认了工作区实现，不能确认已合入发布。后续提交 / 发版前需要重核。

### 修订项 2：vault 级跨进程锁

**方向正确，应先于 GitLab 后端独立完成。**

收紧三点：

- 锁必须随实际 vault 定位；写死 `~/.keyring/secrets.json.lock` 与新增 team vault、
  自定义目标目录没有闭合
- 覆盖所有 Store 写入口（account/key/server/cli 等），不能只按 CLI 的 set/delete 枚举
- 同步必须保护完整本地事务（读基线 + 合并 + 更新活动文件 + Git 状态）；仅保护最后
  rename 不够

`flock(2)` 方案要交代非 Unix 构建路径（当前已有 `#[cfg(unix)]` 注释区分处理）。
"锁失败明确报错"可接受，但需区分等待锁与立即返回忙碌，避免无限等待。

### 修订项 3：personal vault / team vault

**概念合理，D1 三件套隔离方向正确，但路径契约未闭合。**

- 决策点 2 指定 `_remote/secrets.json` 为活动文件，Store 当前把 `master.key` 与
  `secrets.json` 固定同一 root；指到 clone 内 master.key 也会入仓
- 首次 sync 不能复制整个 personal vault；迁移"源：本地默认 file 库 + D1 三件套"
  应明确：个人库用于读取连接凭据，还是记录也参与迁移。后者重新打开个人凭据
  共享风险

### 修订项 4：团队初始化与 master key

**部分吸收，"两条流程"尚未形成完整契约。**

- 空团队库没有已知密文，如何完成首次建库及成员加入
- 导入已有 vault 时不得替换不匹配的现存 key，否则已有记录失去解密能力
- `KEYRING_MASTER_KEY` 优先于文件；验证导入文件成功不代表后续 Store 使用该 key
- 成员退出后重加密 + 重发 key 只能保护后续状态，不能撤回已下载的旧密文和旧 key；
  若要撤销旧凭据的实际使用权，还需轮换凭据本身——这是现有安全承诺的边界

### 修订项 5：一次性迁移

**加密处理正确，迁移安全契约仍未吸收完整。**

- v1 已要求但 v2 没明文列：目标已有数据拒绝覆盖、逐条解密与记录集合核对、失败
  可恢复、验收前保留源库
- **`visibility` 是核心风险**：输出字段丢失报告并让用户决定，**不足以允许受限
  记录进入全员可解密的团队库**。权限承接必须在记录共享和生产切换前完成，不能
  等到 v2.6 删除 D1 时才检查

### 修订项 6：visibility / 越权 404 与 D1 退役

**决策点 3、6 有实质改进，但四条门槛不够具体。**

1. 明确权限由谁执行，以及谁能持有全集和 master key
2. "应用层 ACL 演示"必须证明绕不过：受限 agent 若同时持有 clone 和共享 key，
   就能绕过服务端过滤
3. 回归必须包含越权读取、列表泄漏、远程禁写、本机授权，并确认消费者迁移完成
4. 回滚必须说明恢复哪个版本，以及切换后新增 / 更新 / 删除的数据如何处理

跨项目协调未完成时保留原 D1 授权路径是正确决定。

### 修订项 7：提交历史与并发承诺

**审计承诺基本收窄到位，决策点 4 仍未闭环。**

- "每条 push 是一个 commit"不是当前流程能保证的；失败重推、已有本地提交都需定义。
  可承诺"远端已接收的提交历史"，不宜承诺一一对应
- staging 验证后原子替换只保证文件替换完整，不能自动保证本地未提交修改被保留
- 必须说明 staging 基于哪份本地状态、如何合并 dirty worktree，Git HEAD/index 与
  活动 JSON 如何保持一致
- 文本 rebase 仍不等同于按 secret ID 合并；v2 没实质修正这项旧承诺
- `pull --abort` 应只撤销该次操作，不能含糊地恢复整个目录

### 修订项 8：加密范围承诺

**前半句正确，后半句重新引入错误保证。**

方案第 152 行正确说明名称、结构和部分元数据可见，但随后说"团队成员……看不到
pat 值"。团队成员按方案会获得同源 master key，能解密值。**必须删除或限定这句话。**
只有**没有 key 的仓库读取者**不能直接解密。

### 修订项 9：v2.3–v2.6 时间表

**版本顺序基本自洽，权限门槛与退役措辞仍冲突。**

- v2.3 推荐 GitLab、v2.4 对 D1 发弃用警告，应明确不要求尚未完成权限承接的生产
  用户切换
- v2.6 删除门槛应直接指向 P0-2 的四条；当前"见决策点 6"定位不准确
- v1 保留用于审计可以接受，但必须明确冲突时以 v2 为准；"全部吸收"目前不成立
- v1 已指出的逐命令路由、未知后端值拒绝、Git 运行依赖及认证失败处理，v2 仍未覆盖

## 新引入问题（按严重度排序）

1. **P0，迁移路径的权限暴露风险**：迁移可先丢弃权限字段进入共享库，而 ACL 门槛
   只约束后续删除 D1。必须把权限验收前移到共享与切换之前
2. **P0，pull 流程存在数据丢失路径**：若实现按"远端 staging → 验证 → 原子替换"
   执行，未进入 staging 合并的本地修改会被覆盖。需明确合并基线和完整事务
3. **P1，冲突标记误判已实现回归**：已修（先 JSON 解析，失败后再细化检测）
4. **P1，导入流程缺保护**：仅验证一条远端密文后接受 key，没有约束已有目标库
   及环境变量优先级
5. **P1，安全表述错误**："团队成员看不到值"与共享 master key 直接矛盾
6. **P1，回滚承诺缺失数据语义**：停止双写后 D1 会落后；"立刻切回"可能丢失更新
   或恢复已删除的旧凭据，不能仅靠保留旧库兑现

## CortexOS 治理层关注

Codex 在做 v2 review 时跑了 `cs rule security_boundary / coding_conventions / flow`，
把 kyvault 放回 CortexOS 治理框架：

- **`cs rule security_boundary` §1** 明确写"密钥真源：D1 `secret_vault`"—— D1 是
  CortexOS 当前的密钥真源，不是 kyvault 项目的"我决定撤就撤"的事
- 修订方案里"撤 D1 后端"的措辞需要更克制：**kyvault 是给 CortexOS 提供 D1 适配
  器的工具之一**，撤 kyvault 的 D1 适配器不等于撤 CortexOS 的 D1 真源
- 真正需要走的是：**CortexOS 治理层增加 GitLab 作为可接受密钥真源**，
  `security_boundary` 改写"密钥真源：D1 `secret_vault` 或 GitLab 私有仓"，
  kyvault 提供新后端 + 提供迁移工具，CS 大脑按需切换

## Commit 拆分建议

- **P0-1 独立 PR：建议**（已包含冲突误判修复 + 关键测试 + main/doctor 适配）。
  它能独立降低现有风险，不应等待 GitLab 后端
- **P0-3 不建议捆绑同一 PR。应独立提交，并作为 GitLab 后端的前置条件**

## 一句话总结

**P0-1 已堵住主要覆盖路径，但冲突误判需先修；v2 还需补齐权限迁移、同步事务
和 key 路径契约；建议先独立交付 P0-1（误判已修），再交付 vault 锁。**