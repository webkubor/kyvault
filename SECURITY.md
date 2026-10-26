# Security Policy

本项目管理的是 API Key、Token、密码和服务器凭据 —— 所以「发现漏洞」与「发现普通 bug」不是一回事。
下面写清楚这个项目实际的安全边界、能做什么、不能做什么，以及怎么报告问题。

## 支持范围

| 版本 | 支持 |
|---|---|
| 2.6.x | ✅ |
| < 2.6 | ❌ 只修已知漏洞，不做功能修复 |

## 这个项目保证什么、不保证什么

**保证（可验证的实现事实）**

- 凭证用 **AES-256-GCM** 认证加密落盘；每条凭证独立随机 Nonce。
- 根密钥 `master.key` 是本地生成的 32 字节随机数，经 `SHA-256(Base64Decode(master.key))` 派生 AES 密钥。
- 文件后端的加解密**完全离线**，不产生网络请求。
- `master.key.enc` 权限为 `0600`；写入用跨进程排他锁 `.lock` 防并发冲突。
- `ky run` 只把值注入指定子进程，不会主动将明文传给模型；子进程本身仍可读取或输出该值。

**旧版 Auth Guard 兼容实现不构成密钥保护。** 它从公开 `.pub` 文件派生 AES key，并未验证 SSH 私钥持有权；TOTP 校验也未接入敏感命令。拿到 `auth.json`、`master.key.enc` 和绑定的公钥即可重算该 key。旧 `setup` / `add` 默认停用；确有旧格式迁移需求时才显式设置 `KYVAULT_ALLOW_INSECURE_LEGACY_AUTH=1`。

**本地 file / Git Sync 的信任边界**：每个能取得 master key 的主体可解密整个 vault。GitLab SSH key/deploy key 控制仓库访问，不控制单条凭据；`visibility` 和 `scopes` 当前是元数据，`get` 尚未据此拒绝访问。服务器 IP 台账字段本身不参与授权；broker 另有来源 IP/CIDR 策略。

**实验性 SSH broker**：只有在受信管理员维护的专用 OpenSSH 账户、`restrict/from=/command=`、可信程序和 0700 密库目录下，forced command 才把登记的 SSH key 与设备 ID 绑定，服务端再检查实际对端 IP、精确 ref、期限与撤销。`SSH_CONNECTION` 并非独立的身份认证，不能把 `broker serve` 作为普通远程 shell 入口。Agent 不获得 master key，服务端审计写失败则拒绝取用；日志仍可被本机管理员修改，不能叫不可抵赖。撤销只阻止后续请求，不追回已取走的静态凭据。`principal`、`--as`、`KY_ACTOR`、`$USER` 都是标签；Mac/Windows 主设备签名审批尚未实现。

**不保证（请不要依赖这些）**

- **`ky get` 会输出明文。** 它是设计如此 —— 拿明文的方式就在你自己终端里。
  如果你在 AI 助手的会话里执行它，输出会进入模型上下文。**给 AI 用凭证请用 `ky run`**，
  它把值注入子进程环境，不经过模型。
- **子进程可以读到、也可以打印注入的值。** Kyvault 不提供沙箱，也不过滤子进程输出。
  注入的凭证进了子进程的环境变量，那条子进程及其后代就都能看到。
- **不是所有操作都离线。** `ky check`（API 校验与余额查询）、`ky update`、
  `kyvault gitlab sync`、以及可选的 D1 后端都会联网。
- **不替你管理 GitHub 权限。** 存进库里的 Token 有多大权限，就是它有多大权限。
  过期或泄露了要你自己去平台轮换 —— Kyvault 不会替你改云端权限。

## 威胁模型：它防什么、不防什么

**防**

- 使用 `ky run` 可避免先把凭据明文写入命令/模型上下文；不保证子进程输出不会泄漏，也不保证第三方训练数据政策。
- file 后端只将 AES-GCM 密文同步到 Git 时，不会主动将凭据值作为仓库内容。
- 若攻击者仅有凭据密文、没有 master key 或旧 Auth Guard 的可重建材料，AES-GCM 提供加密保护。
- `check` 与 `inspect` 能提供检查/过期线索；目前没有自动调度、自动通知或自动轮换保证。

**不防**

- 已经在你机器上以 root / 你的用户身份运行的恶意进程。
- 你自己执行 `ky get` 后把明文贴到了聊天窗口、issue、日志里。
- 已被泄露的凭证 —— Kyvault 能让你换新值，但不能追溯谁看过旧值。
- 子进程主动外传它拿到的密钥。
- 本地页面（`ky ui` / `ky add`）开着时，已经拿到页面完整地址（含随机 token）的本机程序。
  token 挡的是本机浏览器里打开的其他网页：它们能请求 `127.0.0.1`，但猜不到路径，
  也拿不到跨站读取许可。2.6.0 及更早的 `ky ui` 没有这层保护，见 CHANGELOG。

## 报告漏洞

**请不要开 public issue。** 漏洞细节公开会让利用变得容易。

用 GitHub 的私密漏洞报告功能：
[Security → Report a vulnerability](https://github.com/webkubor/kyvault/security/advisories/new)

或者发邮件到 GitHub 账号里公开的那个地址（见 <https://github.com/webkubor>）。

请在报告里写清：

1. 受影响版本（`ky --version` 的输出）。
2. 复现步骤。
3. 影响面（需要什么前置条件：本地访问？物理接触？还是仅逻辑错误）。
4. 你建议的修复方向（如果有）。

### 我们的承诺

- 收到后会尽快首次回应，告诉你是否复现、初步判断，以及是否已有缓解措施。
- 确认是漏洞后给出修复时间表，并在修复 released 后公开致谢（除非你要求匿名）。
- 修复发布后会在 `CHANGELOG.md` 里记一笔。

> 具体响应时限请以我们实际的回复为准 —— 这份文档不写死一个我们无法保证的时长。

## 关于依赖漏洞

本项目依赖 `rustls`（不用系统 OpenSSL），依赖面刻意保持很窄。
`cargo audit` 可以查依赖公告：

```bash
cargo install cargo-audit
cd rust && cargo audit
```

发现依赖有高危公告时，请开 issue 或直接按上面的方式报告 —— 这类问题我们通常会立刻处理。

## 负责任的披露

我们不设赏金，但会认真对待每一份报告，并公开致谢。
请给你自己留出合理的披露窗口（90 天），我们会在这个窗口内修复并发布。
