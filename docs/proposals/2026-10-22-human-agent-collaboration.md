---
audience: [human, agent]
tags: [kyvault, architecture, identity, device, audit, broker, credentials, observability]
priority: high
status: draft-for-review
date: 2026-10-22
supersedes: 2026-09-23-v3-design-skeleton.md
---

# kyvault 协作安全模型：从本地密库到人类与 Agent 的共同治理

> 产品方向：让人类知道「谁、在哪台设备、用哪个平台账号、拿了哪种凭据、做了什么」；让 Agent 只取得当前任务被授权的凭据；让维护者持续知道凭据是否有效、是否过期、是否该撤销或轮换。
>
> 推广承诺必须落在可验证的安全边界上。当前 Git 同步模式适合一个可信组共享密钥；多台服务器 Agent 的逐条授权需要 broker，不能把共享 master key 分发到每台服务器。

---

## 1. 用户场景与结果

| 用户问题 | 产品需要回答 | 结果 |
|---|---|---|
| 哪些人和自动化在用？ | Principal：人、设备、服务 Agent 的稳定身份 | 看到身份来源、责任人、状态、最后活动时间 |
| 从哪里访问？ | Device / Host：工作站、服务器、Agent runtime、环境、IP 观察值 | 识别设备、撤销设备；IP 作为附加网络条件 |
| 多个平台有几个账号？ | Account：平台、租户/组织、登录名、profile | 一平台多账号，一账号下挂多条不同类型凭据 |
| 密库里有什么？ | Credential：凭据类型、用途、作用域、所有者、有效期、状态 | 能盘点、筛选、发现过期/重复/无人负责凭据 |
| Agent 为什么能取？ | Grant：身份 × 设备 × 凭据 × 动作 × 环境 × 时间 | 默认拒绝，授权可解释、可到期、可撤销 |
| 它做过什么？ | AuditEvent：认证、授权、读取/注入、检查、轮换、拒绝 | 能追查请求、命中哪条规则、结果是什么 |
| 凭据现在还活着吗？ | HealthCheck 与 RotationPolicy | 失效和到期提前发现，不把限流误报为失效 |
| 这台服务最近是否正常？ | 可观测性：最后心跳、版本、策略同步、检查延迟和错误率 | 区分设备离线、凭据失效、权限不足、服务故障 |
| 长期有没有失控？ | Review：重复凭据、过宽 scopes、长期未用、孤儿账号 | 定期复核并给出可执行的收敛建议 |

产品要围绕这九类问题提供一份真源数据，而不是继续增加互不关联的特殊命令。

---

## 2. 领域模型

### 2.1 Principal 与 Device 分开

- **Principal**：真实的人或服务身份，如 `person:alice`、`agent:deploy-prod`。
- **Device**：身份运行所在的设备，如 `device:alice-mac`、`host:prod-01`、`runtime:deploy-42`。
- 人可以登记多台设备；服务 Agent 可以在多台主机上运行；换机器不应改变人的身份。
- Device 至少有不可共享的公钥指纹、类型、责任人、环境、登记/撤销时间、最近心跳和客户端版本。
- 服务器 SSH host key 用于客户端确认「连到哪台主机」；agent client key / mTLS 证书用于服务端确认「是哪台 agent」。这两种 key 不能混为一谈。
- 用户 Mac / Windows 的主设备 key 用于签署设备登记或授权审批；不把私钥复制到服务器。

### 2.2 Account 与 Credential 分开

**Account** 表示平台上的账号/profile；**Credential** 表示可轮换的秘密。一个账号可挂多种凭据，凭据也可标记不同环境。

Credential 至少记录：

- 稳定 ID、平台、Account ID、名称、凭据类型；
- 用途、owner、环境、scopes、可被哪些 principal/device 使用；
- `created_at`、`expires_at`、`last_checked_at`、`last_used_at`、`last_rotated_at`；
- 状态：`active / expiring / expired / invalid / permission_limited / unknown / revoked`；
- 密文位置、轮换方法、检查器类型。

支持类型从 API token、用户名密码开始，逐步扩展到 SSH 私钥、客户端证书、云服务身份、数据库凭据和动态凭据。类型决定检查与轮换能力，不把所有东西都当成一段无差别字符串。

同一平台可有 `personal / work / staging / production` 多个 Account；每个 Account 再挂 PAT、OAuth token、SSH deploy key 等多条 Credential。现有 `profile` 注册表是 UI/CLI 迁移起点，不是最终权限模型。

### 2.3 Grant 是实际的授权规则

```text
Grant = principal + device/workload + credential + actions + environment + validity + approver
```

- 动作采用最小集合：`inject`、`read`、`check`、`rotate`、`admin`；Agent 默认不具备 `read`、`rotate`、`admin`。
- Grant 绑定具体凭据和环境，可设置用途、有效期、次数或短租约。
- 新设备、新 Agent、新生产凭据默认拒绝，须由人类主设备审批或按预先写明的策略批准。
- 注销 Agent 只撤销该 Agent 的 grant；凭据被 Agent 明文取走后若怀疑泄漏，仍要轮换该凭据。
- `visibility` 与 `scopes` 只有被服务端授权路径强制执行后才叫访问控制；在当前本地/Git 模式里，它们只是元数据。

### 2.4 AuditEvent、HealthCheck、Review

**AuditEvent** 至少含认证后的 principal/device、credential ID、动作、allow/deny、命中规则、请求 ID、时间、结果码和来源网络信息。不能把 CLI 的 `--as` 或 `$USER` 当作认证身份；它们只是本地标签。

**HealthCheck** 保存检查器类型、时间、状态、HTTP 分类、耗时、下次检查时间和错误摘要，不保存凭据值或可重放的认证材料。

**Review** 周期扫描：将到期、已失效、长期未用、重复值候选、未绑定 owner、scopes 过宽、Grant 长期未复核等事项汇成清单。重复检查优先使用带密钥的指纹；不把低熵秘密的明文 SHA-256 写入共享元数据。

---

## 3. 三种运行模式与安全边界

| 模式 | 数据位置 | 谁能解密 | 适用范围 |
|---|---|---|---|
| **Local** | 单机本地加密文件 | 能解锁该机 vault 的用户/进程；实质上对该库有全部权限 | 个人、可信本机脚本 |
| **Git Sync** | GitLab/Git 等同步密文和元数据 | 拥有同一 master key 的成员可读全库 | 小团队、成员彼此完全可信、允许全库共享 |
| **Broker** | 可信服务持有 vault root key 和密文；Agent 不 clone 全库 | 服务端按 Grant 解密并只释放该次授权的凭据 | 多服务器 Agent、需要逐条授权/撤销/审计的团队 |

GitLab 私仓、deploy key、SSH clone 权限只控制谁能拉取密文；它们不自动限制解密后的凭据。Git Sync 的 master key 一旦交给 Agent，该 Agent 就有离线解密整库的能力，`visibility=local` 无法阻止。

### IP 白名单是网络条件，不是身份

IP 可做额外限制，不能代替 Principal/Device 身份：NAT、VPN、容器、代理会共享出口 IP，服务器也可能换址。Broker 必须从实际连接端取得来源 IP；只有配置过的反向代理才能提供可信转发地址。授权至少同时验证 agent 证书/签名身份、设备状态、Grant 和 IP policy。

### SSH key 的职责

- SSH **host key**：让客户端确认服务器身份。
- 人类设备 **client key**：证明审批者/操作者持有已登记的私钥。
- Agent **client key / mTLS cert**：证明请求来自已登记的 Agent 实例。
- 任何公开 SSH 公钥指纹都只能标识公钥，不能作为秘密、不能证明私钥持有，也不能直接派生 AES key。

设备身份验证采用带随机数、有效期和 audience 的签名挑战，或标准 mTLS。授权审批由已登记的人类设备签名；私钥不离开原设备。

---

## 4. 推荐协作流程

```text
人类主设备 (Mac / Windows)
   │ 1. 登记并持有个人设备私钥
   │ 2. 审批 Agent 登记与具体 Credential Grant
   ▼
自托管 kyvault broker ── 加密库与 master key 留在可信服务侧
   ▲       │
   │ 3. Agent 使用独立机器身份认证 + 可选来源 IP policy
   │ 4. 请求 secret ref / 指定动作 / 当前任务
服务器 Agent
           5. broker 校验 Grant，释放获准的凭据或短期租约
           6. 服务端记录 allow/deny、credential、device、request ID
```

Agent bootstrap 使用短时、一次性 enrollment code；由人类主设备确认目标 host key、Agent 身份公钥、服务器资产/IP 和用途后签发。Enrollment code 不能变成永久 bearer token。

**推荐的部署模式：**专用 SSH forced command、自托管设备登记、精确到 ref 的 Grant、默认拒绝、IP/CIDR 附加条件、有效期与即时撤销、强制服务端审计。SSH host key pin/known_hosts 验证 broker 主机；SSH 用户私钥验证 Agent 设备身份。当前工作区已加入实验性 CLI broker 原型，距离生产使用还需专用账号/sshd 配置、信任审查、运维流程与真实平台验收，默认不自动改服务器 sshd 或用户 authorized_keys。

边界必须说清：当前原型返回获准的静态凭据给 SSH 客户端，再注入目标子进程；该进程能够读取、保留或外传它。Broker 能限制下一次发放、记录并撤销未来请求，不能追回已复制到内存/日志的秘密。对高风险用途优先给短期 token，或后续让 Agent 调用 broker 代理而不直接得到上游长期凭据。

---

## 5. 场景能力与推广验收指标

| 场景 | 核心能力 | 可度量结果 |
|---|---|---|
| 设备管理 | 登记、最后心跳、版本、停用/撤销、主设备标识 | 每次授权能回答是哪台设备发起 |
| 人类与 Agent 身份 | Principal、设备证书/签名、审批来源、服务责任人 | 未登记 Agent 请求全部拒绝且可解释 |
| 密码与凭据类型 | 类型化 Credential、owner、用途、环境、到期和轮换方法 | 条目有 owner、用途和状态，缺项可被盘点 |
| 审计 | 认证/授权/取用/拒绝/轮换事件、请求 ID | 生产凭据访问有完整的服务端事件链 |
| 可观测性 | Broker 健康、设备心跳、策略版本、请求延迟、错误分类 | 能区分设备离线、策略拒绝、上游故障与凭据失效 |
| 重复性监察 | 定期检查、过期提醒、重复候选、闲置和过宽授权复核 | 到期/闲置/重复问题进入可处理清单，而非只显示数字 |
| 有效性 | 平台检查器、valid/permission-limited/rate-limited/unknown 等状态 | 限流不误判失效，401 与 scope 不足分开 |
| 多平台多账号 | 平台 → Account/profile → 多种 Credential | 同平台账号可并存；`run/use` 明确选择目标账号 |

---

## 6. 分阶段路线

| 阶段 | 交付 | 验收边界 |
|---|---|---|
| **A. 信任说明与本地台账** | 修正 Auth Guard 不准确安全承诺；统一 Account/Credential 元数据；audit、有效期、检查时间、类型、owner 和只读总览 | 不宣称当前 `visibility` 已授权；不声称本地 audit 不可抵赖 |
| **B. 设备与健康管理** | Device/Principal 注册表、心跳、撤销状态；平台检查结果模型；到期/闲置/重复 Review | 不把 IP 当身份；检查默认只读，不自动轮换 |
| **C. Broker MVP** | 专用 OpenSSH forced command 原型：SSH key 设备身份、服务端 `SSH_CONNECTION` IP 检查、精确 ref grant、期限、撤销、强制审计 | Agent 不拿 master key、不 clone 全库；未登记/过期/超范围默认拒绝；部署时须锁定专用账户和 sshd policy |
| **C2. 主设备审批与部署加固** | Mac/Windows 主设备签名审批、一次性 enrollment、sshd 与账户隔离自动检查/验收、操作员审计身份验证 | `--as` 人名标签升级为验证过的审批身份；不支持的部署配置要 fail closed |
| **D. 安全凭据生命周期** | 短期凭据、轮换 provider、上游代理调用、策略化审批、多组织 | 轮换需确认云端成功、旧值撤销和回滚路径 |

Local / Git Sync 继续存在，作为明确的信任组模式；Broker 是多人多 Agent 的授权模式。D1/其他后端迁移应独立决策，不能把删除后端当作身份治理的先决条件。

---

## 7. 当前实现需要纠正的事实

1. 当前 `AuthGuard` 的 `derive_key_from_ssh()` 对**公开 SSH 公钥字节**做 SHA-256，然后作为 AES key；`unlock()` 只读 `.pub`、校验指纹并解密，不会证明持有私钥。这不是私钥认证，也不是可靠的密钥保护。需替换为真正的私密密钥包装/操作系统密钥库方案，迁移期间不能宣传为抗仓库读取者的保护。
2. 当前 `visibility`、`scopes` 没有在 `get_secret()` 中执行，不能向用户承诺条目级访问控制。
3. 当前 `audit.log` 是本地可改文件；`--as` / 环境变量 / git config 的人名可由调用者伪造，只适合本机操作线索。不可抵赖的团队审计要由 broker 在认证后写入，最好追加到受保护的远端日志。
4. 当前 `run` 注入环境变量会让被启动进程及其子进程读到该值；broker 不能阻止已授权进程主动外传。
5. 当前 server `ip` 是加密资产字段，不参与授权判断；IP 白名单必须成为 broker 策略并校验连接来源。

### 需要优先更新的旧 v3 结论

- 删除「Agent 默认走 SSH key 自动解锁 master key」「服务器有 SSH key 就能解锁全库」；改为 Agent 以自身身份请求按 ref 授权，设备私钥只证明身份。
- Recovery code 仅作恢复，不放在同一个 vault 再由 Agent 读取（会形成循环依赖）。Argon2id 只用于低熵 passphrase KDF，不增加随机恢复码熵。
- GitLab 不是 broker；删 deploy key 不能收回已 clone 的密文或已拿到的 master key。撤销需要服务端拒绝未来请求；已泄露凭据要轮换。
- 不把明文 SHA-256 写入共享 metadata 来做完整性声称。AES-GCM 已有认证标签；若要跨密文重复检测，使用 keyed fingerprint 并限制暴露面。
- D1 保留/退役需要独立的兼容与数据迁移评审，不与 Agent 授权架构捆绑。
