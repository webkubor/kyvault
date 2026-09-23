# 更新日志 (Changelog)

所有对 `kyvault` 项目的重大变更都将记录在本文档中。

## [2.4.0] - 2026-09-22

**安全架构重大升级：Auth Guard 多因素授权守卫，master.key 从明文保护升级为加密态保护。**

告别裸文件 `master.key` 时代——通过 SSH 公钥绑定和可选 TOTP（Google 验证器），将 `master.key` 加密为 `master.key.enc`，即使 store 被整体拷走也无法解密。本地 Agent 凭 SSH 公钥零交互自动解锁，日常操作毫无感知。

### ✨ 新增
- **Auth Guard 多因素授权守卫（`kyvault auth`）**：
  - `setup`：交互式首次配置，自动扫描 `~/.ssh/*.pub`，绑定 SSH 公钥并加密 master.key。
  - `add ssh <path>`：追加绑定 SSH 公钥（支持多把，任一把可解锁）。
  - `add totp`：绑定 TOTP（Google 验证器），用于 `master-key --reveal` 等敏感操作的二次确认。
  - `list`：查看已绑定的授权方法、指纹及公钥状态。
  - `remove <id>`：移除一种授权方法（至少保留一个 SSH key）。
  - `disable`：关闭 Auth Guard，恢复明文 master.key 模式。
  - `status`：守卫状态概览（含解锁测试）。
- **`kyvault master-key` 命令**：
  - 查看 master.key 物理路径、文件权限、SHA-256 指纹。
  - `--reveal` / `-r`：显示完整明文密钥（用于备份至 1Password / 离线密码库）。
- **本地极客 Web GUI 图形界面（`kyvault ui`）**：
  - 零外部运行时依赖，内置基于标准库 `TcpListener` 的超轻量本地服务，秒级启动并自动唤起默认浏览器。
  - 极客暗黑风单页界面（SPA），卡片式可视化选择平台、智能推导推荐命名并组装标准 `secret://<platform>/<name>`。
  - 支持直观加密录入、快速检索查看、敏感密码明隐切换、一键复制 `kyvault run` 命令。
- **极客终端配置向导（`kyvault wizard` / `kyvault guide`）**：
  - 全新升级结构化引导：分类选择 → 平台选择 → 用途/环境推荐 → 不回显密文安全输入。
  - 彻底规整命名格式，消除随意命名与分类混乱。
  - 支持从命令行一键打开 Web GUI 进行视觉化操作。
- **全流程极客 TUI 视觉设计（`tui` 模块）**：
  - `kyvault doctor`、`kyvault auth status`、`kyvault auth list` 全面换装圆角边框、ANSI 流光色彩与对齐指示符。
  - 严格支持 `NO_COLOR` 规范与非 TTY 自动降级纯文本。
- **快捷短命令 `ky`**：
  - 提供 `ky` 命令行入口，与 `kyvault` 命令全等价。
- **doctor 自检新增第 4 段 Auth Guard 守卫状态**：
  - 检测 master.key.enc / auth.json 状态、绑定方法数量、明文残留检查。

### 🔧 修复与改进
- **`default_store_dir()` 兼容 Auth Guard 模式**：识别 `master.key.enc` + `auth.json` 组合为已就绪状态，不再因明文 `master.key` 被删除而回落到旧目录。
- **`Cmd::Init` 路径显示修复**：不再硬编码 `~/.keyring/master.key`，显示实际 `store.master_key_path()` 路径。
- **`harden()` 和 `shellexpand_tilde()` 改为 `pub`**：供 auth_guard 模块复用。

### 🗑️ 废弃
- **`~/.keyring/` 旧目录**：不再作为默认 store。已迁移至 `~/.config/kyvault/store/`，旧目录可安全删除。

## [2.3.0] - 2026-09-22

**核心架构决策：本地与远程直接迁移至最新版本，密钥管理彻底去中心化，不依赖任何外部调度系统。**

密钥真源全面收拢并归宿于「本地存储 + GitLab 私有团队仓（`kyvault-store`）」，解密为纯本地离线计算，断网可用；`master.key` 永不入仓、永不经网络下发。外部调用方不再参与密钥生命周期管理与代管，彻底消除因远程依赖、D1 网络抖动或静默重置 master key 导致存量密文孤立的严重事故隐患。

### ✨ 新增
- **GitLab 团队协作后端（`kyvault gitlab`）**：
  - `status`：自检远端 origin 地址、分支跟踪、ahead/behind 提交数及工作区状态，带 `master.key` 绝对排除验证。
  - `pull`：拉取最新密文与元信息，内置 `--autostash` 与冲突自动防护，防止覆盖本地改动。
  - `push`：安全提交并推送 `secrets.json` 和 `meta.json` 到远端仓库，严格拒绝推送 `master.key`。
  - `sync`：一键自动 pull 后 push。
  - `setup <repo-url>`：快速将团队 Git 仓库克隆至 `~/.config/kyvault/store` 并引导带外导入 `master.key`。
- **GitLab 仓库路径默认自动对齐**：
  - `Store::default_store_dir()` 优先使用检测到的 `~/.config/kyvault/store`（若已就绪），无需手动指定 `KYVAULT_STORE_DIR`。
- **`meta.json` 原生旁挂联动**：
  - `list` 自动读取 `meta.json` 补齐 `kind`、`account`、`last4`、`created_at`、`updated_at`、`visibility`、`org`、`scopes`。
  - `set` 和 `delete` 自动同步更新/清理 `meta.json`。
  - `annotate` 命令不再局限于 D1，全面支持本地/GitLab 仓库的元信息修改。
- **跨进程排他文件锁 (P0-3)**：
  - 在 store 根目录下使用 `fs2` 实现跨平台进程文件锁（`.lock`），写操作全流程受互斥锁保护，防止多 Agent 并发破坏。

### 🔒 兼容与平滑过渡
- `d1` 后端代码完整保留，标记为过渡维护模式，`doctor` 提供清晰的团队 Git 仓库引导。
- 外部工具全面退化为普通使用者，通过环境变量 `KYVAULT_STORE_DIR` 或默认目录调用 `kyvault` CLI，不再做数据源中转。

## [2.2.0] - 2026-09-12

围绕一件事:让 kyvault 能独当身份 + 密钥两摊,不再依赖任何外部中转层。

### ✨ 新增
- **`org` / `scopes` 两个正式字段**（`annotate --org --scopes`、`list --org` 过滤）。
  此前机器人的组织归属只能塞在 `account` 备注里 —— 备注是自由文本，
  过滤不了、也校验不了。org 用来回答「哪个公司有哪些机器人」，
  scopes 让 agent 拿到 key 之前就知道自己能干什么。
- **`identity` 视图**：飞书/Lark 机器人凭据是**一对**（app-id + app-secret），
  单独一条谁也用不了。`kyvault identity [--org]` 按前缀聚合回「身份」，
  标出「✅ 成对」= 可直接装进 lark-cli。
- **`d1-setup` 自举**：把连 D1 的三件套存进本地加密库（`~/.keyring`），
  之后裸跑 kyvault 直连 D1，不再需要外部注入环境变量。
  连 D1 的 token 本身就是密钥，本地加密库是它唯一能自举的地方。
- **Windows 支持**：CI 加 `x86_64-pc-windows-msvc`，产物覆盖
  Mac（Intel/ARM）+ Linux（x64/ARM）+ Windows。

### 🐛 修复
- **跨平台编译硬伤**：`store.rs` 无条件 `use std::os::unix`，Windows 上直接
  编译失败。抽出 `harden()`：Unix 收 0600，Windows 继承用户目录 ACL。
  这是此前 CI 一直没有 Windows target 的隐性原因。
- **老库升级不炸**：D1 加列用幂等 `ALTER`，`list` 先 `ensure_schema` 再查 ——
  老库直接 SELECT 新列会整条失败。

### 🔒 兼容
- `from_env()` 改为「环境变量优先，缺了才读本地库」：CI、容器、临时覆盖
  照旧，不破坏任何现有调用方式。

## [2.1.2] - 2026-09-11

装完 2.1.1 做端到端实测时发现的。

### 🐛 修复
- **`connect` 对「块外的旧版规则」只保留、不出声。** 它会把重复的规则副本清成
  一份，但内容变过版本的旧副本精确匹配不上、删不掉 —— 保守不删是对的（猜边界
  可能连用户自己写的内容一起削掉），可一声不吭就意味着同一个文件里两套互相不
  一致的规则并存，agent 读到哪份全看运气。现在会指出文件路径和可搜索的标题。
  实测本机 `~/.clauderules` 里正躺着一份缺了第 4 条规则的旧版。
- **`upsert_block` 的两条路径合成一条。** 原先「有标记块就替换」和「没块才去重」
  是两段独立逻辑，于是去重与旧版检测只在后一条上生效 —— 已经有标记块的文件
  （也就是装过一次之后的所有文件）跑多少次都不吭声。现在统一先抠掉标记块、
  在块外做完所有处理再追加，有块没块走同一段代码。

## [2.1.1] - 2026-09-11

发 2.1.0 之后真装了一次，两个只在「真实安装 / 真实环境」下才看得见的问题当场暴露。

### 🐛 修复
- **`install.sh` 下载失败就直接死。** 实测 `curl: (16) Error in the HTTP2 framing
  layer`，同一个 URL 加 `--http1.1` 立刻成功 —— 这类故障在有代理/企业网关的机器上
  很常见，但用户看到的只是「下载失败」。改成默认先试（带 `--retry 2`），失败退
  HTTP/1.1 再试一次。
- **源码兜底在官方推荐的装法下必然失效。** 它用 `dirname "${BASH_SOURCE[0]}"` 找
  源码，而 README 教的正是 `curl … | bash` —— 那种跑法下 BASH_SOURCE 指向管道，
  仓库根本不在本地，于是报「本机没有 cargo + 源码可兜底」，而本机明明装着 cargo。
  改成：本地有源码就用，没有就现 clone 一份浅拷贝再编，编完删掉。
- **`doctor` 报假警。** 它只查 `CF_API_TOKEN`，而 `d1.rs` 读 token 的口径是
  「先 `CLOUDFLARE_API_TOKEN`、再退回 `CF_API_TOKEN`」。于是在注入前者的环境里
  （部分自动化脚本环境正是如此）doctor 报 FAIL 而实际能用。判据改成与 `d1.rs` 完全一致，
  并打出实际来自哪个变量名。报假警的自检比没有自检更糟 —— 人会学着无视它。

## [2.1.0] - 2026-09-10

### ✨ 新增
- Rust 侧补齐最后 9 个只有 Python 版才有的子命令，两侧命令面完全对齐：
  `check`（21 个平台的 key 有效性验证，顺带查余额）· `providers` · `doctor` ·
  `import`（.env 批量导入）· `server` / `cli`（资产与多 Profile 台账）·
  `update` · `connect` · `wizard`

### 🗑 移除
- **删掉 Python 实现**（`kyvault/`、`tests/`、`pyproject.toml`、`uv.lock`）以及
  CI 里的 python job。命令面已被 Rust 全覆盖，留着只是两份要同步维护的东西。
  `install.sh` 的 `cleanup_python_version()` 保留 —— 老用户升级仍要靠它清掉
  pipx 包 / `/usr/local/bin` 的 python shim / 数据目录下的模块副本。

### 🐛 修复
- `connect` 每跑一次就往 `.clauderules` / Codex `AGENTS.md` 重复追加一份规则：
  幂等判据查的字符串（`Multi-Account`）根本不在被追加的内容里，判据永远不成立。
  改成标记块 `<!-- kyvault:begin/end -->`，有块整块替换、没块追加，并清掉精确
  一致的无标记旧副本（实测某台机上已堆了 4 份）。
- `update` 原本跑 `pip install --upgrade kyvault`，而 PyPI 从未成功发布过，
  那条升级路径一直是死的。改为查 GitHub Release + 交给 `install.sh`。
- `wizard` 录密钥时明文会回显在终端里。改为不回显（关不掉时明确提示）。
  另外它原先写死 file 后端 —— 在注入了 `KYVAULT_BACKEND=d1` 的环境里跑会把密钥
  存到本地而不是真源，人却以为存进去了；现在按当前后端落库并打出后端名。
- README 的 FAQ「需要安装 Python 吗」答案写着「是的，需要」，而紧接的正文说
  「不需要 Python」—— 重写时改了一半。

### 🔒 测试
- 老库兼容改成固定测试向量：`rust/tests/fixtures/` 存一份 Python 1.x 真实写出的
  `master.key` + `secrets.json`，Rust 对着它解。此前那条测试靠现场 import Python
  实现做对照，既把「删 Python」永久卡住，又会在没装 `cryptography` 的 CI 里静默
  跳过（看着是绿的，其实没跑）。那把 fixture master.key 只能解开同目录那份库、
  明文都写在断言里，公开零损失。

## [2.0.0] - 2026-09-10

### 💥 破坏性变更 (Breaking)
- **实现语言换成 Rust，分发换成静态二进制。** 安装方式从 `pip install kyvault`（那条命令其实一直跑不通，见下）改为 `curl -fsSL .../install.sh | bash`，不再需要 Python、也不需要 Rust 工具链。
- **不再发布到 PyPI。** 历史上 `publish.yml` 每次打 tag 都以 `invalid-publisher: valid token, but no corresponding publisher` 失败——PyPI 侧从未配置 trusted publisher，所以包一次都没上传成功，而 README 的第一条命令一直写着 `pip install kyvault`。访客照着第一步走就是 404。这次不补发 PyPI，而是从根上换掉分发方式：Rust 版存在的意义就是不依赖运行时，再挂一条 pip 链条自相矛盾。

### ✨ 为什么重写
不是为了性能，是**消除 Python 环境这个单点故障**。密钥库是所有需要凭据的工作的底座，它只读就等于全线停工；而 1.3.0 那条 curl 兜底修的正是「解释器 TLS 验证整体失效」——那是在给一个不该存在的依赖打补丁。Rust 走 `rustls` 静态链接，根本不碰系统 OpenSSL 那条路径。

### 🔒 数据兼容（存量密钥原地可读，无需迁移）
两套 wire format 完整实现并做成自动化测试：

| | 本地 file 后端 | Cloudflare D1 后端 |
|---|---|---|
| 密钥派生 | `SHA256(base64_decode(master.key))` | `base64_decode(site_config)` 直接用，不派生 |
| 密文布局 | `base64(nonce ‖ ciphertext)` 单字符串 | ciphertext / nonce 分两列各自 base64 |

`tests/interop.rs` 真的起 python3 做双向对照，两套格式各测一遍（Python 加密 → Rust 解得开；Rust 加密 → Python 解得开），再加一条「两套 key 派生不可互换」防止有人图省事把两个函数合并。

实测：读真实 D1 库 162 条、字段名与 CortexOS Go 端完全对齐；同一条密钥两边解密结果 sha256 前 16 位一致、长度一致。

### ✅ 已实现
`init` / `get` / `set`（含 `-` 从 stdin 读，密钥不进 argv）/ `delete` / `annotate` / `list --json` / `platforms` / `run` / `alias` / `account` / `key` / `platform`。

所有核心子命令与输出格式全部对齐，保持极简清晰。

### ⏳ 尚未移植
`import`（.env 导入）、`check` / `providers`（各家 API 校验）、`connect`（kyconnect）。需要这三个的场景暂时仍可用 Python 实现（仓库内保留）。`wizard` 不打算移植——交互式向导要人对着终端敲，与「人只做决策、agent 执行」冲突。

### 🐛 修复 (Fixed)

### 🐛 修复 (Fixed)
- **D1 后端在 Python TLS 失效的机器上不再整个不可用**：`_query` 改为 urllib 优先、传输层失败时自动回退 `curl`。触发它的是一类真实故障——证书链、CA bundle、系统时间全部正常，同一条链用 `openssl verify` 判 OK、`curl` 也连得通，唯独 Python 报 `CERTIFICATE_VERIFY_FAILED`，且跨解释器复现（homebrew python 3.10/3.13/3.14 与 uv 独立构建 3.11/3.12 全中，只有链 LibreSSL 的系统 python 幸免），换版本、重装 openssl、换 CA bundle 均无效。密钥写入是刚需，不能因为解释器的 TLS 坏了就完全写不进去。
  - 回退路径不降低安全性：token 走 `--config` 临时文件（0600）而非 argv（避免 `ps` 泄露），payload 走 stdin 不落盘，且**绝不使用** `-k/--insecure`——换掉的是 HTTP 客户端，不是 TLS 验证。
  - 仅传输层异常才回退；HTTP 4xx/5xx 仍按原样交给 `success` 判断，免得把业务错误伪装成网络问题。

## [1.3.0] - 2026-08-11

### ✨ 新增功能 (Added)
- **AI 智能体一键连接 (`kyvault connect`)**：支持自动发现本地与全局 AI 编码配置目录（如 Gemini config），一键注入 `kyvault-ops` 技能规范与规则。同时自动在当前工作项目区注入 `.cursorrules` 与 `.copilotinstructions`，使 IDE 编码助手能安全地使用本地密钥别名，明文零泄露。
- **加密服务器台账 (`kyvault server`)**：支持服务器公网 IP、root 密码、云服务商与月租成本的加密存储，并提供第一公民级 CRUD 命令行界面。
- **CLI 客户端多 Profile 令牌管理 (`kyvault cli`)**：支持为同一个 CLI 工具在不同账户名/Profile 下维护不同的 Token，解决多开发环境鉴权证书漂移问题。
- **URI 二级路由映射**：在 `get_secret` 统一入口增加了对 `secret://server/<hostname>/<field>` 与 `secret://cli/<cli_name>/<profile>` 的路由映射。

### ⚙️ 变更 (Changed)
- 将命令行版本号升级为 `1.3.0`。
- 重构了 `store.py` 内部的数据隔离结构，将保留字 `_servers` 与 `_clis` 进行系统级隔离，不影响原扁平平台账户与密钥列出。
