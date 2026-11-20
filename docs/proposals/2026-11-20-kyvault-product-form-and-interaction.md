---
audience: [human, agent]
tags: [kyvault, product, all-in-one, interaction, security, masking, local-model, roadmap]
priority: high
status: draft-for-review
date: 2026-11-20
scope: 只读调研 + 方案设计。未改代码、未发布、未读取任何明文凭据、未动真实密钥库
---

# kyvault 产品形态与交互方案：all-in-one 密钥台账

> 面向 owner 的方向稿。回答四件事：**收什么**（边界）、**怎么用**（交互形态）、
> **怎么不漏**（安全模型）、**怎么变成一个能拿 2 万星的开源项目**（路线）。
>
> 全文标注 **[事实]**（本轮在本机/线上实测读到的）与 **[推断]**（基于事实的判断，未实测）。
> 没有实测过的兼容性一律不写成结论。

---

## 0. 结论速览

**结论 1 · all-in-one 的边界是「五类来源 + 三层角色」，不是「所有密码」。**
要收的是：浏览器保存的密码、macOS 钥匙串条目、各平台/各服务器的 API Key 与账号密钥、
项目 `.env`、SSH 私钥与云厂商 token。**不收**的是：支付卡、一次性验证码，
以及任何需要「每次重新认证」的东西——它们进台账只会制造假的掌控感。

**结论 2 · 交互形态推荐「CLI + 桌面 GUI（Tauri）双轨」，CLI 是机器接口，桌面是人的主入口。**
桌面版已经有真实实现（剪贴板历史、片段、凭据库视图、Agent 录入收件箱），
不是从零造。纯 CLI 无法承载「导入 300 条 Chrome 密码并逐条确认」这种批量人工动作；
浏览器扩展要跨浏览器内核重做权限模型，成本与收益严重失衡。

**结论 3 · magpie 那两条思路可以直接落成两个具体机制，但要落在 kyvault 自己的位置上。**
脱敏（magpie 网关出口遮蔽）→ 落成 kyvault 的**出口遮蔽**：导出 / 分享 / 审计面板 / 日志里的密钥片段自动遮蔽。
本机小模型分类（magpie 用 `classifier: typesafe/jev-latest` 与 ONNX 敏感实体检测）→ 落成 kyvault 的
**导入期分类器**：自动判别凭据类型与所属平台、判定重复项、给命名建议，**明文永不进模型上下文**。

**结论 4 · 当前最大的安全缺口是主密钥明文落盘。**
代码里 `master.key` 是 0600 的**明文文件**；旧的 Auth Guard 走公开 `.pub` 派生，
源码注释自己写明「不构成私钥保护、不得据此授权远程 Agent」。
主密钥换成 OS 钥匙串 / Secure Enclave 包裹，是 all-in-one 之前必须先补的地基。

**结论 5 · 2 万星缺的不是功能，是分发、信任与一个一句话说得清的定位。**
现状：GitHub 4 star / 1 fork、crates.io 总下载 16 次、无 badges、文档站已上线但没有社区。
最小可行定位是 **「AI Agent 时代的密码管理器：别名注入，模型永远看不到明文」**——
这个定位在 2026 年看起来是空位（**推断**：主流密码管理器的注入命令是「给人用的密码管理器附带的 agent 功能」，
没有把 agent 边界当第一性建模对象；此处未做竞品穷举调研），但要拿星，README 必须让人 30 秒内看懂，且必须有真安装量。

---

## 1. 现状盘点（全部 [事实]）

### 1.1 仓库、分发、体量

| 项 | 实测值 | 读取来源 |
|---|---|---|
| 仓库路径 | `/Users/webkubor/dev/agent-infra/kyvault` | 目录 |
| 主语言/形态 | Rust CLI（`rust/`，`rust/src/*.rs` 31 个模块）+ Tauri 2 桌面版（`desktop/`，前端是无构建的原生 HTML/CSS/JS）+ 静态文档站（`docs/`） | 目录树 |
| 本机 CLI 版本 | `kyvault 2.8.2`（`~/.local/bin/ky`、`~/.local/bin/kyvault`） | `ky --version` |
| GitHub | `webkubor/kyvault`，MIT，**4 star / 1 fork / 0 open issue**，创建 2026-06-13，最近 push 2026-11-16，20 个 topics，homepage `webkubor.github.io/kyvault` | GitHub REST API |
| crates.io | crate `kyvault`，max_version **2.7.0**，共 2 个版本，**总下载 16 次**，创建 2026-10-21，5 个 keywords、2 个 categories，**0 badges** | crates.io API |
| 版本落差 | 本机 2.8.2 / crates.io 2.7.0 → **发布落后本地一个版本** | 同上 |
| 现有对外素材 | `docs/` 静态站（`docs/index.html` + 2 张截图 assets），GitHub Pages 已上线且返回 200；GitHub **20 个 Release**；`docs/outreach/`（awesome-rust / HN 的投稿草稿，未发布） | 目录 + GitHub API + HTTP 探测 |

### 1.2 已有能力 vs all-in-one 缺口

**已有的（不重复造）**

| 能力 | 现状 | 位置 |
|---|---|---|
| 加密存储 | AES-256-GCM，两套 wire format（file 后端 `base64(nonce‖ct)`；D1 后端 ciphertext/nonce 分列），派生 `SHA256(base64(master.key))` | `rust/src/crypto.rs` |
| 后端 | 本地 file 后端（`KYVAULT_STORE_DIR` > `~/.config/kyvault/store` > `~/.keyring`）+ Cloudflare D1 后端 + GitLab 密文同步层 | `store.rs` / `d1.rs` / `gitlab.rs` |
| 别名 | `aliases.json` 明文映射 alias→ref，**不含密钥**；位于库根目录（`~/.config/kyvault/store` 或 `~/.keyring`，见 `alias.rs::default_location`）；`ky run --env TOKEN=secret://…` 注入子进程环境变量（另有 `--profile` 按账号批量注入） | `alias.rs` / `main.rs` |
| 唯一明文出口 | 全命令中**只有 `get` 打印明文**；`set` 支持 `-` 从 stdin 读（不进 ps / history）；长密钥走 `--file` | `main.rs` / `SKILL.md` |
| 审计 | 本地 `audit.log` 0600，记「谁/何时/动了哪条」的元信息，**不记明文/密文/last4**；`KY_AUDIT=0` 关闭，`KY_AUDIT_SYNC=1` 才入 Git | `audit.rs` |
| Agent 录入请求 | Agent 只写**元信息**（平台/名称/账号/别名），人把密文粘进本机界面；30 分钟 TTL | `requests.rs` |
| 远程 Agent 授权 | `ky broker`：OpenSSH forced-command，设备公钥 + 来源 IP 白名单 + 逐条 ref 授权，`broker-audit.jsonl` | `broker.rs` |
| 离线盘点 | `ky inspect`：默认只读元信息；`--duplicates` 才在内存解密比对（HMAC），不落盘 | `inspect.rs` |
| 只读面板导出 | `ky panel`：自包含静态 HTML，**无明文/密文/last4**，带 `noindex`；设计上刻意与可写的 `ky ui` 分开 | `panel.rs` |
| 本地 Web GUI | `ky ui`：127.0.0.1 随机 token 路径 + Host 校验 + 只收 JSON + 不发 CORS 头 | `ui.rs` |
| 桌面版（Ky） | Tauri 2：剪贴板历史（独立密钥、0700/0600）、命名片段（带版本）、凭据库视图（增删改查/复制 ref/复制明文需显式确认 + 30 秒剪贴板清理）、Agent 收件箱、privacy mask、i18n 中英、托盘、⌘⇧Space | `rust/src/desktop.rs` + `desktop/src-tauri/` + `DESIGN.md` |
| 凭据有效性检查 | `ky check` 对 20+ provider 做真实 HTTP 校验；`health.json` 只存 ref + 状态 + 时间 | `providers.rs` / `health.rs` |
| 平台预设 | 5 类 25 个平台预设（ai/code/cloud/server/account），含 cloudflare、github、gitlab 等 | `category.rs` |
| `.env` 导入 | `ky import --file .env --dry-run`，跳过空值，默认建别名 | `import_env.rs` |
| Chrome 密码导入 | **已存在但只到「手动 CSV」级别**：`tools/import-passwords.sh` + `tools/import-chrome-csv.py`（走 Chrome 官方导出 → stdin → `ky set`，同名默认跳过，支持 `--dry-run`/`--overwrite`） | `tools/` |
| AI 助手接线 | `ky connect` 用标记块（`<!-- kyvault:begin -->`）幂等写入 Claude/Codex/Cursor 的规则与技能包 | `connect.rs` |
| 自检 | `ky doctor`：运行环境/后端 → 文件与权限 → 解密 → Auth Guard → AI 助手对接 | `doctor.rs` |

**缺口的（all-in-one 必须补）**

| 缺口 | 证据 |
|---|---|
| **macOS 钥匙串导入完全不存在** | 全仓 `grep -e chrome -e keychain -e 钥匙串` 只命中 `tools/import-passwords.sh`（Chrome CSV）与一条历史待办；**没有任何钥匙串读取代码** |
| **主密钥明文落盘** | `store.rs::init_master_key` 直接 `fs::write(master.key)` + `harden()` 置 0600；全仓无 `keychain` / `security-framework` 依赖 |
| **旧 Auth Guard 明确不安全** | `auth_guard.rs` 头注释：「从公开 `.pub` 派生 AES key……没有证明调用者持有 SSH 私钥……不得描述成 MFA、私钥认证」 |
| **无本地模型 / 无分类器** | 全仓无模型推理依赖；`category.rs` 的平台分类靠预设 + 关键词规则（`import_env.rs` 的猜平台规则） |
| **无出口遮蔽机制** | `panel.rs` 是「人工挑字段不导出」（人工降级），不是自动脱敏 |
| **导入来源只有 2 个半** | `.env`（有）+ Chrome CSV（有，但需手工导出）+ 钥匙串（无）+ SSH key（只能手 `set --file`） |
| **无 MCP server** | `docs/outreach/README.md` 自述：「kyvault source inspected here exposes a CLI, no verified MCP server」 |

---

## 2. all-in-one 的边界与「唯一真源」角色

### 2.1 收什么（分层清单）

| 层 | 来源 | 收什么 | 现状 |
|---|---|---|---|
| L1 日常账号 | Chrome / Brave / Chromium 保存的密码 | url + username + password | 手动 CSV 已有 |
| L1 日常账号 | macOS 钥匙串通用密码条目 | 服务名 + 账号 + 密码 | **无** |
| L2 机器凭据 | SSH 私钥（`-----BEGIN … KEY-----`） | 私钥全文（PEM/OpenSSH） | 只能手 `set --file` |
| L2 机器凭据 | 服务器 root 密码、跳板机密码 | 明文密码 | 已有 `ky server` 资产台账 |
| L3 平台凭据 | 各平台 API Key / Token / PAT / OAuth | key 字符串 | 已有（25 个平台预设 + `check`） |
| L3 平台凭据 | Cloudflare / GitLab / Supabase / 阿里云 等云厂商 | token / access key + secret | 已有平台预设；云厂商双因子（key+secret）需扩展 kind |
| L4 项目凭据 | 项目 `.env` | KEY=VALUE 全量 | 已有 `ky import` |
| L4 项目凭据 | CI/CD secret、`.npmrc`、`.pypirc`、`docker login` | 账号 + token | **无**（分散在各项目） |
| 元数据 | 账号/项目/服务器台账 | 结构化 | 已有 `registry.rs` + `accounts.json` |

**不收（明确排除，写进产品边界）**

- 支付卡号 / CVV —— 风控与合规成本远大于收益；
- 一次性验证码、TOTP 种子 —— 会过期、会失效，收进来只会制造「我以为统一了」的错觉；
- 任何需要**每次重新人工确认**的凭据（银行、支付）—— 那不叫台账，叫负担；
- 已经在目标系统里可随时轮换、且轮换成本为零的共享密钥（公开 CI 匿名 token 之类）。

### 2.2 kyvault 作为唯一真源的角色分工

这是 all-in-one 最关键的一句话：**kyvault 是唯一真源，但不是唯一存储**。

| 角色 | 能写 | 能读 | 拿到什么 |
|---|---|---|---|
| **人（owner 本人）** | 全部：导入、录入、修改、删除、轮换、授权 | 全部（含明文） | 明文（仅在显式操作后） |
| **本机 agent（Claude Code / Codex / Hermes / OpenMausBot）** | 只写**元信息**（走 `ky request` 收件箱）；不直接写密文 | 元信息 + `check` + `health` | **别名 + 注入值**（`ky run --env`，明文只存在于子进程环境变量，不回显、不落盘） |
| **远程 agent（服务器上）** | 无 | 仅被显式授权的 ref，且动作限 `inject` | 注入值（经 broker forced-command 注入，值不进 agent 上下文） |
| **第三方应用（IDE 插件、CI）** | 无 | 无 | 只能拿到自己专属的、被限定 scope 的 token |

三条铁律（[推断]，但与现有代码注释一致）：

1. **明文只进两个地方**：人的输入框（`ky request` 收件箱）、子进程环境变量。绝不进模型上下文、绝不进审计日志、绝不进 Git。
2. **别名是永久句柄，明文是临时值**。`secret://` ref 与 alias 是稳定的，平台 token 会轮换，轮换时 ref 不变、别名不变，上下游脚本无需改。
3. **台账可以投影，不能倒灌**。GitLab 同步的密文是投影，不是备份策略的替代；master.key **绝不入仓**（`gitlab.rs` 已有该护栏的 push 前强制检查）。

### 2.3 导入流程设计（只写设计，未实测，不动真实凭据库）

#### 2.3.1 Chrome / 浏览器密码

**方案 A（推荐，先做这个）：官方导出 CSV → 本机导入器**

- 路径：Chrome → 设置 → 自动填充 → 密码管理器 → ⋯ → 导出密码（**Chrome 自己弹 Touch ID 授权**，用户完成，kyvault 不参与）→ 产出一份明文 CSV → 交给本机导入器 → 逐条预演 → 确认写入 → **立刻提示删除 CSV**。
- 现状基础：`tools/import-chrome-csv.py` 已实现「域名→平台名归一（去 www./端口，取主域）、密码走 stdin 不进 argv、同名默认跳过、`--dry-run` 预演、脚本自身不打印密码」。**把它 Rust 化并并入桌面版即可，不需要重写导入逻辑。**
- 关键增量（all-in-one 真正缺的那部分）：**导入前的批量确认界面**——一次性展示「将新增 N 条 / 跳过 M 条已存在 / 冲突 K 条」，冲突项让用户选「保留现有 / 覆盖 / 另存」，并且**默认不覆盖**（现有脚本已经是这个默认，保留它）。
- 授权模型：Chrome 侧授权由 Chrome 自己向用户要，kyvault 不绕过；kyvault 侧只读 CSV 文件。
- 风险：CSV 在磁盘上是明文。缓解：导入后提示路径并可提供 `ky import --purge-source`（删源文件，需二次确认）。

**方案 B（进阶，需实测后再决定）：直读 Chrome 的 Login Data**

- 技术路径：只读打开 `~/Library/Application Support/Google/Chrome/Default/Login Data`（SQLite，**必须以只读模式打开，且不能让 Chrome 因锁文件/WAL 写入而丢数据**）→ 取 `origin_url / username_value / password_value` → 解密。
- 解密口令来自钥匙串条目 `Chrome Safe Storage`（`security find-generic-password -w -s "Chrome Safe Storage" -a "Chrome"`），密码字段在部分版本是 AES-128-CBC + PBKDF2。
- **[推断，未实测]** 新版 Chrome（macOS）已把部分密码条目的存储从「整库加密」改为「逐条钥匙串条目」，两条路径会并存；具体版本行为必须实测后再写代码。
- **强烈建议**：不做 B，或把 B 定位为实验性开关（`--experimental-direct-chrome`），默认关闭。B 的收益是「少一步手工导出」，成本是版本漂移 + 碰浏览器数据库的破坏风险 + 钥匙串 ACL 授权弹窗。

#### 2.3.2 macOS 钥匙串

**[推断，未实测]** 有三条路，风险递增：

| 路径 | 做法 | 风险 |
|---|---|---|
| **A. 定向白名单（推荐）** | 用户在桌面版里勾选要收的 service（如 `Chrome Safe Storage`、`gh`、`glab`、`git`），逐条 `security find-generic-password -w -s <service> -a <account>` 拉值 | 几乎为零：用户自己声明范围，系统钥匙串自己弹授权 |
| B. 全量枚举 | `security dump-keychain` 列条目 → 过滤 → 逐条取 | `dump-keychain` **输出里含密码**，必须管道直连、即时丢弃、不落盘不进日志；且噪声大（几百条无关注解） |
| C. 读钥匙串 db 文件 | 直读钥匙串 db | 不推荐：格式非公开、权限与 SIP 边界复杂 |

**建议**：只做 A。桌面版里做成「钥匙串导入向导」——列出可识别 service 的候选（用 `security find-generic-password -a <name> -s <service>` 逐个探测常见名字），用户勾选，逐条预演后写入。SSH 私钥单独走 `set --file`（不进钥匙串）。

**解密入口的权限提示**：钥匙串条目第一次访问会弹系统授权框（「想要访问钥匙串里的……」），这是**好事**——它就是 macOS 的原生授权模型，不要试图绕过。

#### 2.3.3 `.env` 与项目文件

- 现状：`ky import --file .env --dry-run` 已有，规则是「跳过空值、去成对引号、默认建别名」。
- 增量设计：新增 `ky import --scan <dir>`（默认跳过 `.git`、`node_modules`、`dist`、`venv`），对多个 `.env` 做一次预演面板；**`.env.example` / `.env.template` 一律跳过**（那是模板不是凭据）。
- 一个必须加的护栏：**预演面板默认只显示 KEY 名、来源文件路径和推测平台，明文永不显示**；只有用户勾选某一条时才解密看一眼。

#### 2.3.4 导入冲突处理（三条规则）

1. **同名默认不覆盖**（现有 CSV 导入器已是此行为，保持）。理由：`ky set` 会用新值覆盖旧值，批量导入一旦覆盖，用户手动改过的密码会被打回旧值，而且没有 undo。
2. **冲突必须逐条可见**：预演面板分三桶——`新增` / `已存在跳过` / `值不同需裁决`。第三桶逐条显示「旧值 last4 vs 新值 last4」，由人选。
3. **导入整体可撤销**：每次批量导入生成一个 **import batch id**，写入 `audit.log` 并保留一份「导入前后的 ref → 密文引用」的短期快照（密文快照，0600，默认留 7 天），提供 `ky import rollback <batch-id>` 回滚该批次。

### 2.4 导入流程总览（新增部分为 [推断]）

```text
选择来源 ──┬── 浏览器官方导出 CSV ──┐
           ├── 钥匙串定向白名单 ────┤
           ├── 项目 .env 扫描 ──────┤→ 导入器（本地）→ 预演面板
           └── SSH key / 云厂商 ────┘        │
                                             ├─ 本地小模型分类器（类型/平台/重复候选/命名建议）
                                             ↓
                          逐桶确认（新增 / 跳过 / 冲突裁决）
                                             ↓
                          写入密文库 + 建别名 + 写审计 + 记 batch id
                                             ↓
                          提示清理源文件（CSV / .env 备份）
```

**本机小模型分类器见 §4.2；明文永不进模型上下文，是这个环节的硬约束。**

---

## 3. 交互形态：三选一

### 3.1 方案 A · 纯 CLI + 别名注入

**用户路径**：`ky import` / `ky set` / `ky list` / `ky run --env K=secret://… -- cmd`。

- 优点：零 GUI 维护成本；天然可脚本化；agent 友好（`ky` 已在 Claude/Codex/Hermes 的技能包里）；SSH 主机上也能用。
- 缺点：批量人工确认几乎做不了——「导入 300 条密码并逐条裁决」在终端里是折磨；普通用户（非开发者）完全无法使用。
- 密钥暴露面：低。但 `ky get` 会把明文打到终端，**落在 scrollback / 终端日志 / tmux buffer 里**——这是需要专门处理的点（见 §5.5）。

### 3.2 方案 B · CLI + 桌面 GUI（Tauri 2）—— **推荐**

**用户路径**：桌面 App 做「人」的主入口（导入向导、逐条确认、剪贴板、请求收件箱、盘点面板）；CLI 做「机器」接口（`run`/`get`/`set`/`check`）；二者共用同一个 Rust 核心库与同一个本地密文库。

- 优点：桌面版**已经存在**（Tauri 2 + 剪贴板历史 + 片段 + 凭据库视图 + Agent 收件箱 + privacy mask + i18n + 托盘 + ⌘⇧Space），不是从零造；桌面天然是「人在旁边」的安全场景，钥匙串授权框、Touch ID、二次确认都能自然落地；剪贴板历史 + 片段 + 凭据库三合一，天然就是「all in one」的日常形态。
- 缺点：Tauri 桌面应用要维护 3 个平台的安装包与签名/公证（**当前仅 macOS 已验证，Windows/Linux 未验证** [事实]）；分发成本高于 CLI。
- 密钥暴露面：中。桌面进程内解密，但已有约束：前端永不持有明文、只用 `textContent` 不用 `innerHTML`、`localStorage` 只存 `ky.lang` / `ky.toolbarCollapsed`（`DESIGN.md` 明确）。

### 3.3 方案 C · CLI + 浏览器扩展 / 网页端

**用户路径**：浏览器插件在登录页自动填充；或网页控制台。

- 优点：填密码这件事浏览器里做最顺；可跨设备。
- 缺点（**决定性**）：
  - **安全模型冲突**：网页端 = 明文离开本机 = 违背 kyvault 的立身之本。现有 `panel.rs` 的注释已经把这条逻辑写死了：「把它开在公网上等于把 master.key 的保护全部作废」。
  - **浏览器扩展不可能是唯一真源**：Chrome 扩展的存储有配额、有同步风险，而扩展本身要拿到明文才能填——等价于把钥匙串放到浏览器里。
  - **跨浏览器内核**：Chrome / Safari / Firefox 的密码存储与授权模型完全不同，扩展 API 不统一，要做三套。
  - 已有 `ky ui`（本地 Web GUI）已经把「网页形态」的能力做过了，且已带 token/Host/CORS 三道防护，扩展能带来的增量远小于成本。
- 结论：**不做**。真要做浏览器侧，只做「**不读明文的插件**」：插件只负责在 kyvault 侧登记「这个站点对应哪个别名」，填充动作由桌面 App 完成（需要一条本机 IPC）。

### 3.4 三案对比

| 维度 | A 纯 CLI | B CLI + 桌面 GUI（推荐） | C 扩展 / 网页端 |
|---|---|---|---|
| 批量导入 + 逐条确认 | 不可用 | 桌面原生 | 扩展读不到本地库 |
| 日常取用（填密码、跑命令） | 命令行 | 剪贴板 + 快捷键 + 面板 | 插件填，但明文出本机 |
| 密钥暴露面 | 低（但 get 会进 scrollback） | 中（进程内，前端不持明文） | **高**（明文进浏览器/远端） |
| agent 友好度 | 最高 | 高（同一后端） | 低 |
| 跨平台成本 | 最低（单二进制） | 中（3 平台安装包 + 签名） | 高（3 内核 × 3 平台） |
| 已有资产复用 | 100% | 高（Tauri 已有骨架） | 低（`ky ui` 已覆盖类似定位） |
| 与「2 万星」叙事 | 弱（工具味重） | **强（可截图、可视频、可看界面）** | 强但安全叙事崩 |

### 3.5 推荐理由与分期

**推荐 B，CLI 与桌面是同一个产品的两个面，不是两条产品线。**

理由：owner 的原话是「用户一定用起来要方便，同时安全非常重要」。
CLI 满足「方便」里的**机器面**（agent、脚本、CI、SSH 主机）；
桌面满足「方便」里的**人面**（导入裁决、粘贴密文、找条目、临时片段）；
而「安全」在两者上都靠同一条边界保证：**明文只出现在人的输入框和子进程环境变量里**。
桌面版甚至比 CLI 更安全——它有显式确认、有 30 秒剪贴板自动清理、有系统钥匙串授权框。

分期（[推断]）：

- **P0（地基）**：主密钥换 OS 钥匙串包裹 → 桌面版加「导入向导」→ 钥匙串定向白名单导入 → Chrome CSV 导入 Rust 化并进桌面。
- **P1**：导入期分类器（本地小模型）、批量回滚、导出自动脱敏、盘点面板。
- **P2**：Windows / Linux 桌面包、MCP server、浏览器「站点→别名」轻插件（不读明文）。

### 3.6 桌面 GUI 现在到什么程度（[事实]，逐条评估）

| 能力 | 现状 | 对 all-in-one 的缺口 |
|---|---|---|
| 凭据库视图 | 列表 / 新增 / 替换 / 删除 / 复制 ref / 复制明文（**显式确认** + 30 秒剪贴板清理） | 缺**导入向导**（批量 + 逐条裁决 + 冲突分桶） |
| 剪贴板历史 | 有，独立密钥加密，0700/0600，最长可配 | 缺「从剪贴板历史一键入台账」 |
| 命名片段 | 有，带版本 | 片段与凭据的关系未定义（片段明文存哪、是否入库） |
| Agent 收件箱 | 有（`requests.rs` 对接），含「锁定期间到达、解锁后补弹」的修复 | 缺批量处理与「待处理计数」入口 |
| 隐私遮蔽 | 有（frontend 只显示后端返回的已遮蔽内容） | 与 magpie 那种「出口自动遮蔽」不是一回事，深度不够 |
| 平台 | macOS 已验证；**未签名 / 未公证**，Windows / Linux 未验证 | 分发是拿星的硬门槛 |
| i18n | 中英双语已有 | 英文文档 / Issue 模板是拿星必需项，目前缺 |

**结论**：桌面版是一个「能用、有品味、但只覆盖台账日常读取」的骨架。
all-in-one 缺的不是重写，而是**把导入这一个动作做成一等公民流程**。

---

## 4. 借鉴 magpie 的两条做法

### 4.1 脱敏

#### 4.1.1 magpie 现在是怎么做的（[事实]）

- **出口遮蔽**：magpie 在请求离开本机前做模式识别与凭据遮蔽，落盘在
  `~/.config/magpie/masking/`（`2026-11-20.jsonl` 明细 + `checked-2026-11-20.json` 按调用方汇总）。
- **命中量**（`checked-2026-11-20.json` 实测）：

  | 调用方 | looked | masked | 遮蔽率 |
  |---|---|---|---|
  | openmausbot | 1700 | 1685 | 99.1% |
  | hermes | 852 | 850 | 99.8% |
  | dsh | 533 | 318 | 59.7% |
  | claude | 1430 | 466 | 32.6% |
  | antigravity | 111 | 77 | 69.4% |
  | billion-context | 554 | 136 | 24.5% |
  | codex | 218 | 1 | **0.5%** |
  | curl / Go-http-client / Python-urllib / Gateway-Probe | 389 / 82 / 36 / 255 | 0 | 0% |

- **明细格式**（`2026-11-20.jsonl` 实测）：每条记录 `key / t / route / what / agent / provider / model / n / values[]`，
  `values[]` 每项含 `p`（占位符，形如 `{{TERM_3rxn7p3h}}`）、`k`（类别，如 `TERM`）、
  `b`（body 类型）、`h`（命中首字符）、`l`（长度）、`n`（命中次数）、`a`（动作，如 `warn`）。
- **配置面**（`~/.config/magpie/settings.json` 实测）：`redactKinds`（`ip / home / userhost / bucket` 开关）、
  `redactWords`（当前是 `["王恩博"]`）、`redactWordsAnyCase`、`redactActions`（`TERM → warn`）、
  `redactModelScore`（`{"astrlink-guard": 0.95}`）、`redactModelName`、`redactModelOff`（当前 **true**，即本机此刻是关的）。
- **本机小模型敏感实体检测**：`~/.cache/magpie/astrlink-guard/7216b6a3b897/` 下有
  `model_int8.onnx` + `tokenizer.json` + `coreml/` —— 即**本地 ONNX/CoreML 量化模型**，阈值 0.95。

#### 4.1.2 哪些做法可以变成 kyvault 的机制（[推断]）

| magpie 做法 | kyvault 对应机制 | 落点 |
|---|---|---|
| 出口遮蔽 + 占位符替换 | **导出/分享自动脱敏**：`ky panel`、新的「导出条目为 .env」「导出给同事」动作，默认把值替换成 `{{alias}}` 占位符，导出物是「引用清单」而不是「密钥清单」 | `panel.rs` 扩展 |
| `values[].h / l / n` 命中元数据 | **审计里的「命中即遮蔽」**：审计记「这条 ref 的值出现在第几次请求、命中长度」，但不记值本身 | `audit.rs` |
| 遮蔽率按调用方统计 | **kyvault 的「暴露面看板」**：按 agent / 工具统计「本机有多少次把值送进了上下文」，用来验证「别名注入是否真的生效」 | 新增 `ky exposure` |
| `redactKinds` / `redactWords` 白名单 | **值级遮蔽规则**：某些 ref 的值永远不显示明文，只显示 last4 | `model.rs` / `credui.rs` |
| `astrlink-guard` 本地 ONNX 敏感实体检测 | **入库前扫描**：新写入的密钥值先过一遍本地检测器，判断「这看起来像私钥 / 卡号 / 高熵串」，命中就提示「这是不是不该进台账的东西」 | 导入器 |

#### 4.1.3 magpie 方式的局限（[推断]，这一条很重要）

1. **遮蔽是「事后」的**——请求已经带着明文走到了网关，才被替换。magpie 解决的是「别让明文进云端模型」，**没有解决「明文别出现在本机进程 / 日志 / 终端 scrollback」**。kyvault 的 `ky get` 明文进终端 scrollback 这件事，magpie 帮不上忙。
2. **遮蔽率强烈依赖调用方**——实测 codex 遮蔽率 0.5%、claude 32.6%、curl / Golang / Python 客户端 0%。说明**只要有一个不经过 magpie 的出口，遮蔽就不是保证**。kyvault 不能把「出口遮蔽」当安全保证，只能当纵深防御的一层。
3. **遮蔽是「识别式」的，天然有漏与误伤**——高熵 base64、JWT、多行 PEM、一次性 token 都可能漏判；反过来正常的长句也可能被误判。
4. **占位符往返需要还原端**——magpie 在回程把占位符换回原值，这要求它同时持有原文，安全性依赖于「网关自己可信」。kyvault 若做占位符导出，还原责任不能交给导出目标。
5. **本机此刻 `redactModelOff = true`** —— 说明这套遮蔽**默认是关的**，owner 并未把它当作强依赖。kyvault 借鉴时应把它设计成**默认开、可关**，而不是默认关。

**结论**：kyvault 的第一道防线**不能**是脱敏，必须是「值压根不进上下文」（`ky run` 注入）。脱敏是第二道、第三道防线。

### 4.2 本机小模型分类

#### 4.2.1 magpie 现在是怎么做的（[事实]）

- **路由分类器**：`~/.config/magpie/providers.json` 中 10 个 routing group 配了 `"classifier": "typesafe/jev-latest"`；
  规则形如 `{"use": "xiaomi/mimo-v2.6-pro", "intent": "复杂推理、架构设计…"}` / `{"use": "xiaomi/mimo-v2.6-flash", "intent": "常规编码实现…"}`。
  `typesafe` 是独立 provider（`~/.cache/magpie/models/typesafe.json`，base `https://api.typesafe.ai/v1`，模型 `jev-latest` / `jev-preview`）。
  **[推断]** `jev-latest` 是一条小尺寸本地/低价模型，用于把用户意图归到少数几个 `intent` 桶，再由规则选模型。
- **结构化任务画像而非关键词匹配**：AgentCompass 插件 README 明确写
  「It routes on a profile, never on the words of a task. The profile comes from the caller (`body.profile`) or from a classifier, a small model you configure.」
  —— 即**小模型产出结构化画像**，路由基于画像，不基于原文关键词。
- **本地模型白名单（privacy-gate）**：`privacy-gate.middleware.js` 的 `localModels` / `localProviders` 白名单机制，
  语义是「只有白名单里的本地模型才允许接收二进制/敏感载荷；不配置就默认全部拒绝（fail-closed）」。
  注释里写明设计意图：「guide the agents toward local extraction」 —— **把提取动作推回本机做**。
- **敏感实体本地检测**：`astrlink-guard`（ONNX int8 + CoreML，阈值 0.95），见 §4.1.1。

#### 4.2.2 这套做法怎么落到 kyvault（[推断]）

核心原则（与 magpie 一致，但 kyvault 的表述更严格）：**小模型做判定，原始密钥永不进模型上下文。**
具体落点三个，都放在**导入期**：

| 落点 | 输入给模型的东西 | 模型输出 | 为什么必须本地 |
|---|---|---|---|
| **凭据类型判别** | `ref` 的名字 + 平台候选 + 值的**结构特征**（长度、字符集、是否有 `-----BEGIN`、前缀）——**绝不含值本身** | `kind`（API Token / 密码 / 私钥 / Cookie / OAuth Refresh…）+ 置信度 | 类型判错会把密钥挂到错的平台下，之后 `get` 找不到，人会以为导入失败又导一遍 |
| **重复项判定** | 只出**HMAC 指纹**（已有能力：`inspect.rs` 用 HMAC 在内存比对） | 重复组聚类 + 建议保留哪条 | 值可能完全一样但 ref 不同（同一把 key 存在三个地方），这是 all-in-one 最常见的脏数据 |
| **命名建议** | 域名 / service 名 / KEY 名 / 已有别名表 | 建议的 `secret://` ref 与 alias | 命名不统一 = all-in-one 变成大杂烩 |

**必须写死的三条工程约束（[推断]，但与 privacy-gate 的 fail-closed 思路一致）**：

1. **本地模型不可用时，行为是「不分类」，不是「猜一个」**。没有分类器 = 走确定性规则（关键词 / 前缀正则），并明确告诉用户「这是规则判断，不是模型判断」。
2. **分类结果永远只是「建议」，必须有人确认**。模型说「这是 Cloudflare API Token」不等于就存成 Cloudflare Token。
3. **模型进程拿不到明文**。所以判别只能吃「特征」（长度 / 前缀 / 结构），不能吃「值」。这会牺牲一部分准确率——这是刻意的代价，不是缺陷。

### 4.3 两条借鉴的汇总落点

```text
导入期 ─┬─ 特征提取（长度 / 前缀 / 结构 / HMAC 指纹，不含明文）
        ├─ 本地小模型分类器 → 类型 / 重复组 / 命名建议（本地、无网、fail-closed）
        └─ 确定性规则兜底（无模型时）
                    ↓
             预演面板（人确认）→ 写入密文库
                    ↓
出口期 ── 导出 / 分享 / 审计 → 自动脱敏（占位符 + last4 + 命中元数据）
```

---

## 5. 安全模型

### 5.1 主密钥与解锁

#### 现状（[事实]）

- 本地 file 后端：`master.key` 是**明文文件**，权限 0600，位于 `~/.config/kyvault/store` 或 `~/.keyring`；
  文件后端派生 `SHA256(base64_decode(master.key))` 作为 AES-256-GCM 密钥。
- 环境变量 `KEYRING_MASTER_KEY` 优先于文件（`store.rs:180`）。
- 旧 Auth Guard：`master.key.enc` + `auth.json`，**从公开 `.pub` 派生**，源码自述不构成保护。团队 GitLab 后端则是「密文入仓、master.key 不入仓」。
- 全仓**没有**任何 macOS Keychain / Secure Enclave 依赖。
- 历史待办已记录同一件事（`.workbuddy-ai/memory/2026-10-22.md`）：「把 master key 改为 OS 密钥库包裹（macOS Keychain / Windows DPAPI / Linux secret-service 或 systemd-creds），废弃 `.pub` 派生路径」。

#### 目标形态（[推断]）

| 档位 | 机制 | 解锁体验 | 适用 |
|---|---|---|---|
| **L0 默认** | master.key 明文 0600（现状） | 无感 | 单人单机，可信环境 |
| **L1 推荐默认** | master.key **加密存盘**，密钥由 **macOS Keychain**（`security` / `security-framework`）包裹；首次设置时钥匙串条目由用户授权创建 | 开机后钥匙串自动放行（同一用户、已解锁登录钥匙串）→ 实际上也是「无感」 | **所有人** |
| **L2 强** | L1 + 解锁需二次确认（Touch ID / 钥匙串 ACL） | 每次解锁一次 | 共享机器 / 团队场景 |
| **L3 极致** | Secure Enclave 签名解锁（用设备私钥签名挑战，门禁放行 master key 派生），broker 场景复用 `broker.rs` 已有的设备公钥与 IP 白名单机制 | 解锁需设备在场 | 高价值库 / 远程 agent |

**兼容性硬约束（[事实]）**：`crypto.rs` 的注释明确写着两套 wire format 必须与 Python 版和 CortexOS Go 端**字节级兼容**。
所以改造方向必须是**只换 master key 的存放与开锁方式，不动 AES-256-GCM 的密文布局**——
即 `KEYRING_MASTER_KEY` 仍可由 OS 钥匙串在运行时喂进来，密文一条都不用重写。
这一点是幸运的：L1 改造是**纯钥匙管理改造**，不涉及数据迁移。

#### 建议

P0 就做 L1：`master.key` 内容改为密文，包裹密钥存 OS 钥匙串；保留 `KEYRING_MASTER_KEY` 作为 CI / 无人值守的显式后门（并在 `doctor` 里明确标注「该模式为无人值守模式，不受 OS 保护」）。
同时提供 `ky auth migrate --to os-keychain`（与历史待办一致），且**必须先备份再迁移**。

### 5.2 审计日志

现状（[事实]）：`audit.log` 本地 0600，记元信息不记明文 / 密文 / last4；默认**不**入 Git，`KY_AUDIT_SYNC=1` 才提交。

增量（[推断]）：

- **批次维度**：批量导入 / 导出记 `batch_id`，支撑 §2.3.4 的回滚。
- **拒绝也要记**：`broker` 拒绝、导入时被分类器标为「疑似高危」被拦、用户撤销授权 —— 失败路径的审计比成功路径更重要。
- **导出单独一档**：导出动作在审计里要单列（谁、什么时候、导出了哪些 ref、是否含明文），因为导出是唯一能突破「明文不出本机」的合法动作。
- **同步节奏**：`KY_AUDIT_SYNC=1` 意味着审计入 Git 明文可见（虽然不含密钥值，但含 ref 名与操作者）。团队场景建议审计走独立通道而非主库仓，或加密后再同步。

### 5.3 导入冲突 / 导出二次确认 / 误操作回滚

| 场景 | 规则 | 依据 |
|---|---|---|
| 导入遇同名 | **默认跳过，不覆盖**；冲突项逐条裁决 | 现有 CSV 导入器已是此默认 [事实] |
| 导入整体回滚 | `ky import rollback <batch_id>`，7 天内可用 | [推断] |
| 导出明文 | 二次确认 + 必须显式 `--include-secret`（默认导出只有引用）；导出的文件名带时间戳；写审计 | [推断] |
| 导出到团队 | 导出物默认是**密文引用清单**（`secret://ref`），不含值；需要值时走 broker 的逐条授权 | 契合 `panel.rs` 的既有立场 [事实] |
| 删除 | `ky delete` 先软删（进回收站，7 天），再物理删；`ky delete --purge` 立即物理删并记审计 | [推断] |
| 覆盖写库 | `ky set` 覆盖前保留旧值快照（短期），支持 `ky set --undo` | [推断] |
| 密钥轮换 | `ky rotate`（[推断]，需新建）：新值写入 + 旧值标记 `revoked` + 审计 + 通知下游 | [推断] |

### 5.4 给 agent 的别名边界

**必须写进产品的边界（[推断]，多数有现有代码支撑）**：

1. **默认拒绝**：`ky run` 只接受显式列出的 ref，不支持「给我这个平台的所有 key」。
2. **别名不是权限单位**：alias → ref 的映射在库根目录的 `aliases.json` 里明文存着，**别名本身不授予任何权限**；授权由 broker / 收件箱 / 桌面确认决定。
3. **值不进上下文**：`ky run` 把值注入子进程环境变量；如果目标命令会把值打出来（例如 `env`、`echo $TOKEN`），**那明文还是会出现在 agent 的输出里**。产品应提供 `ky run --redact-stdout`（子进程输出流经遮蔽再回传）作为默认推荐，并在文档里明确「不要用 `env` / `set` 这类会打印全部变量的命令」。
4. **远程 agent 更严**：broker 已有逐条 ref 授权 + IP 白名单 + 设备公钥 + forced-command；远程路径**只允许 `inject`，不允许 `read` / `rotate` / `admin`**（与 `docs/proposals/2026-10-22-human-agent-collaboration.md` 的 Grant 模型一致）。
5. **Agent 不能自己造别名**：别名由人创建或由导入向导批量创建；agent 只能通过 `ky request` 声明「我需要什么」，由人在本机界面批准。

### 5.5 红线：永远不允许的操作

> 这一节是安全模型的硬边界，建议原样进产品文案与 SKILL 包。

1. **永不明文进模型上下文**——不允许任何默认路径把值写进 agent 的对话、prompt、tool result。
2. **永不让 master.key 进 Git / 进仓库 / 进 CI 变量 / 进团队同步**——`gitlab.rs` 的 push 前强制检查保留并扩展到所有 push 路径。
3. **永不把 `ky get` 的输出重定向到文件 / 日志**——只允许人眼看；`get` 的输出应带一次性提示「这行会被终端保留」，并推荐 `ky run` 替代。
4. **永不在审计、面板、日志、截图、Issue、提交信息里写明文或 last4**（`panel.rs` 已明确排除 last4，保留这条）。
5. **永不批量覆盖已存在的条目而不逐条确认**。
6. **永不静默降级**：分类器不可用 → 走规则并标注；钥匙串拒绝 → 报错不跳过；导入失败 → 不写半截数据。
7. **永不把「未实测的兼容性」写成承诺**——尤其 Chrome / 钥匙串直读路径。
8. **永不自动轮换**——轮换必须有人按下。
9. **旧 Auth Guard 不得被描述为 MFA / 私钥认证 / 密钥保护**（沿用 `auth_guard.rs` 的原话；也不要让它继续当默认路径）。

---

## 6. 2 万星路线的现实判断

### 6.1 差距清单（[事实] + [推断]）

| 维度 | 现状（事实） | 拿 2 万星需要（推断） |
|---|---|---|
| 星标 | **4 star / 1 fork**，建仓 2026-06-13（5 个月） | 见 §6.3，量级是 5000 倍 |
| 分发 | crates.io **总下载 16 次**（2026-10-21 发布，两个版本），0 badges；GitHub 有 20 个 Release（下载量未知） | 稳定安装通道（brew / scoop / curl 一行装）+ 签名 + 校验和 + 更新器（`update.rs` 已有骨架） |
| 平台 | macOS 单平台 | macOS / Windows / Linux 三平台（**Windows 是 agent 用户的最大盘**，Linux 是服务器侧） |
| 安装体验 | 未知 | `curl \| sh` 一行装 + 首屏 demo |
| 文档 | 有静态站 `webkubor.github.io/kyvault` + 2 张截图 | 英文 README + 30 秒可懂的 demo GIF + 架构图 + 对比表（vs 1Password CLI / Bitwarden CLI / Doppler / Infisical / SOPS） |
| 社区 | 无 Issue、无 PR、无 discussion | CONTRIBUTING / CODE_OF_CONDUCT / SECURITY.md / discussions / 模板 |
| 信任 | 无签名、无公证、无第三方审计 | **代码签名 + 公证 + 至少一次第三方安全审计或公开的安全模型文档** |
| 差异化 | 定位是「本地加密密钥库」 | 需要一句话独占的定位（§6.2） |
| 故事 | 无 | 需要一个「为什么现在」的故事（Agent 时代） |

### 6.2 最小可行差异化定位（[推断]）

**候选 A：「AI Agent 时代的密码管理器 —— 别名注入，模型永远看不到明文」**

- 空白判断（[推断]）：1Password CLI 有 `op run` 做注入，Bitwarden 有 `bw run`，但它们的定位仍是「给人用的密码管理器，附送一个给 agent 的注入命令」；
  **没有一个工具把「agent 边界」当作第一性建模对象**——没有别名 / 明文的双层模型、没有「人写元信息 / 人粘密文」的录入通道、
  没有「值不进上下文」这条贯穿全链路的纪律。而 kyvault 已经有：`ky run` 注入、`requests.rs` 的元信息 / 密文分离、broker 的逐条授权、审计。
- 竞品风险（[推断]）：1Password / Bitwarden 的既有用户会问「我为什么不直接用现有工具」。所以差异化不能只靠「给 agent 用」，要靠**「本地优先 + 明文永不出本机 + 全量归集」**这个组合。
- 可讲的故事：一个人有 6 处凭据散落（钥匙串、Chrome、5 个 .env、3 台服务器），想统一管理但**不愿意把主密码交给任何云**。

**候选 B：「零信任凭据台账 + 逐条授权的远程 agent 代理」**

- 差异化更强（broker + 设备公钥 + IP 白名单 + forced-command 是很少见的组合），
- 但**教育成本高**：普通开发者不理解 broker，也不会用。**不适合作为 2 万星的主叙事，适合作为深度文档的护城河。**

**建议（[推断]）**：**A 做定位，B 做差异化深度**。README 主打 A（人人能懂），文档站第二屏讲 B（专家认可）。

### 6.3 三阶段路线（[推断]）

**阶段 0 · 0–3 个月：把「能用」变成「敢推荐」**

- 主密钥换 OS 钥匙串（L1）+ 迁移命令 + doctor 升级；
- 导入向导（Chrome CSV Rust 化 + 钥匙串定向白名单 + .env 扫描）进桌面版；
- 导入期本地分类器 + 预演面板 + 批次回滚；
- 导出自动脱敏；
- 三平台安装包 + 签名 + 公证（macOS 先做，Windows 跟上）；
- 英文 README 重写（一句话定位 + 30 秒 GIF + 对比表 + 一行安装）、SECURITY.md、CONTRIBUTING、Issue / PR 模板、discussion 分类。

**阶段 1 · 3–9 个月：把「单点工具」变成「生态位」**

- MCP server（`docs/outreach/README.md` 已自述缺失）——让 agent 用标准协议取别名；
- 公开的凭据分类 / 平台目录（开源、可贡献），既服务分类器训练也服务生态；
- 团队模式：GitLab / 自建同步的**加密组**与**逐条授权**落地（broker 文档化 + 部署脚本）；
- 第三方集成：GitHub Actions / Vercel / Cloudflare / Docker 的官方用法与模板；
- 真实用户反馈驱动的迭代节奏（对标 `docs/outreach/` 里已经准备好的 awesome-rust / HN 草稿，先把这两个入口打通）。

**阶段 2 · 9–18 个月：把「工具」变成「标准」**

- 一次外部安全审计 + 公开报告；
- 跨平台包管理渠道（brew tap / scoop bucket / apt / nix）；
- 与 1Password / Bitwarden / Infisical 的公开对比与迁移工具（「一键迁入」是拉新最强的钩子）；
- 插件 / 集成生态；
- 社区化：模板库、示例库、贡献者计划。

**现实判断（[推断]）**：

- 2 万星不是「做完全部功能」的奖励，是**分发 + 定位 + 信任**的奖励，功能只需要做到「导入 + 注入 + 审计 + 跨平台」这一条线足够扎实。
- 星标曲线通常在**「一个具体痛点被一个具体的人解决」**之后才起来。对 kyvault 来说，那个「具体的痛点」最可能是：
  「我想让 Claude Code / Codex 用我的 API key，但不想把 key 写进配置文件、也不想让它进对话记录。」
- **不做的清单**：不做自建云同步（放弃服务收入、换来巨大的运维与安全负担）、不做团队席位计费（v2 阶段不谈商业化）、不做密码管理器式的密码自动填充（战场在 1Password 手里，不要打）。

### 6.4 与 owner 其他目标（博主 / AI 音乐）的关系

（[推断]，属产品判断，非技术结论）

- kyvault 走「2 万星开源项目」这条线，与「知名博主」和「AI 音乐领头羊」**不冲突但不同频**。
- 它的价值在于**给内容和技术影响力提供「可验证的硬资产」**：一个别人能装、能审、能用、能量 2 万星的项目，比任何一篇内容都更能证明能力。
- 它同时是**技术内容的天然选题源**：安全模型、别名注入、Agent 边界、导入分类器，每一项都能拆成一篇有信息量的内容 —— 内容侧（小红书 / 推特）与工具侧共用一份素材。
- **判断**：kyvault 应该被当作「证明能力的主资产」来投入，优先级高于「再造一个能赚钱的产品」，因为它同时喂饱成长（技术深度）、学习（新领域）、IP（内容素材）三条线，商业化反而是最晚该考虑的。

---

## 7. 需要 owner 拍板的问题

按影响面排序。每条给出选项与默认建议，**默认建议不自动执行**。

| # | 决策点 | 选项 | 我的建议 |
|---|---|---|---|
| 1 | **主密钥存储** | 保持明文 0600 / 立刻改 OS 钥匙串包裹 / 钥匙串 + Secure Enclave | **立刻改 OS 钥匙串包裹**（L1），Secure Enclave 留 L3 |
| 2 | **导入范围** | 只做 Chrome CSV / Chrome CSV + 钥匙串 + .env 扫描 / 再加钥匙串全量 | **CSV + 钥匙串定向白名单 + .env 扫描**；钥匙串全量不做 |
| 3 | **是否做 Chrome 直读** | 不做 / 实验开关 / 正式支持 | **不做**（版本漂移 + 碰浏览器 DB 的破坏风险），最多留实验开关 |
| 4 | **导入时是否覆盖同名** | 跳过 / 覆盖 / 逐条裁决 | **逐条裁决，默认跳过** |
| 5 | **小模型分类器** | 用本机现成小模型 / 打包一个专用小模型 / 纯规则 | **先纯规则 + 可插拔接口**，等导入量起来再上模型（避免为一个还不存在的规模写模型） |
| 6 | **本地模型是否允许联网下载权重** | 允许一次下载 / 只用已在本机的模型 | **允许一次下载但默认离线优先**；分类器不可用时 fail-closed 走规则 |
| 7 | **导出策略** | 默认含明文 / 默认只含引用 | **默认只含引用**，含明文需 `--include-secret` + 二次确认 |
| 8 | **桌面版平台优先级** | 只做 macOS / macOS+Windows / 三平台 | **macOS + Windows**（Windows 是 agent 用户大盘；Linux 后续） |
| 9 | **是否做 MCP server** | 做 / 不做 | **做**（`docs/outreach/README.md` 自述缺失，是 agent 生态的标准接口） |
| 10 | **仓库归属（GitLab vs GitHub）** | 代码真源 GitHub / GitLab / 双轨 | 面向拿星 → **GitHub 为公开真源**；GitLab 保留为团队内部运营与密文同步仓（不混代码仓库，避免 GitLab token / 密文仓的权限面污染开源仓） |
| 11 | **命名** | kyvault / ky / 换名 | 保留 **kyvault**（仓库名、crates.io 名已占），`ky` 作为短命令；产品对外可叫「Ky」（桌面版已用此名），README 统一写法待定 |
| 12 | **商业化时点** | v2 就不谈 / 有用户后再谈 | **有 1k star 或明确团队付费需求后再谈**，现在谈会污染定位 |

---

## 8. 本方案的边界声明

- 本轮**未改任何代码**、未发布、未推送、未动真实密钥库。
- 本轮**未读取任何明文凭据**：Chrome 密码库、macOS 钥匙串条目、kyvault 密文库均未读取内容；仅确认了 Chrome / Brave / Chromium 配置目录存在（`~/Library/Application Support/…`）与 `security` CLI 存在，用于判断导入路径可行。
- **未实测**的兼容性一律标注为推断，集中在：Chrome 新版密码存储格式、macOS 钥匙串直读、Secure Enclave 集成、Windows / Linux 桌面分发。
- 引用的 magpie 数据来自本机 `~/.config/magpie/` 与 `~/.cache/magpie/` 的只读读取，以及
  `/Users/webkubor/dev/third-party/magpie-plugins/packages/` 下插件源码与 README。
- kyvault 数据来自本机仓库只读查阅、GitHub REST API、crates.io API。
