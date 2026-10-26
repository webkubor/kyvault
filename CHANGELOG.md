# 更新日志 (Changelog)

所有对 `kyvault` 项目的重大变更都将记录在本文档中。

## [Unreleased]

### `ky ui` 跨站读取明文漏洞修复（Security）

旧版 `ky ui` 对所有响应返回 `Access-Control-Allow-Origin: *`，读明文的 `/api/reveal` 没有任何鉴权，端口又固定为 8765。
**只要 `ky ui` 开着，本机浏览器里打开的任意网页都能用 JS 请求 `127.0.0.1:8765`，列出全部条目并逐条读出明文。**
绑 `127.0.0.1` 只挡得住别的机器，挡不住本机浏览器里运行的别家网页。实测：旧版对带 `Origin: https://evil.example` 的请求返回全部 304 条元信息，`text/plain` 的 reveal 请求返回 200。

- 地址改为 `http://127.0.0.1:<port>/<每次启动随机 token>/`，不带 token 一律 404。
- Host 只认 `127.0.0.1` / `localhost`（挡 DNS rebinding）。
- POST 只收 `application/json`（跨站必须先预检），不再返回任何 CORS 头；加 `X-Frame-Options: DENY`、`Cache-Control: no-store`。
- 台账里的 ref / 备注改为转义后渲染，备注不能再注入脚本。
- 升级前开过 `ky ui` 的，建议回想当时是否同时浏览了不可信网页；有疑虑就轮换重要凭据。

### `ky add`：浏览器录入凭据，先验证再写库（Added）

终端粘贴密码容易带进空格、大小写难核对。`ky add` 打开一次性本地页面，验证通过才加密写入。

- **三种模式**：邮箱授权码 / 应用专用密码、邮箱登录密码、API Key。`--email` / `--provider` / `--login` 预填并直达对应模式。
- **自动规范化**：授权码和 API Key 去掉全部空白、全角空格与零宽字符，大小写原样保留；输入时实时预览长度与末四位，长度不符或混入非 ASCII 字符会提示。登录密码只去首尾空白，中间空格保留。
- **找旧条目**：按账号备注里的邮箱精确匹配（不做子串匹配），可覆盖的条目置顶并默认选中；同账号的其他平台凭据只展示不可选。
- **验证**：邮箱授权码用 IMAP 993 登录验证（代理普遍封 SMTP 端口，993 通常放行；直连失败自动走 `HTTPS_PROXY` / macOS 系统代理的 CONNECT 隧道）；API Key 复用 `ky check` 的接口验证。`invalid`、`unknown`、限流等任何非 `valid` 结果都**不写库**。
- **邮箱登录密码**：网页登录有验证码无法在线验证，改为两次输入一致才保存，有大写锁定提示。**一个邮箱只保留一条登录密码**：新的覆盖旧的，同邮箱其他重复的登录密码条目在保存时一并删除（确认框会列出），删除写审计。
- **两类邮箱密码互不覆盖**：授权码不能写进登录密码条目，反之亦然；也不能把 A 邮箱的密码写到 B 邮箱的条目。
- **一次性服务**：随机 token、Host 校验、只收 JSON POST、无 CORS 头、没有任何读明文接口；点「完成」或空闲 15 分钟自动退出。
- 写入后记录 `check` / `set` 审计，并把验证结果写入 `health.json`（登录密码不写，因为没有在线验证）。

### `ky check email <名字>`：复验邮箱授权码（Added）

- 对库里已存的邮箱授权码 / 应用专用密码做 IMAP 登录验证，邮箱地址取条目账号备注里的地址；状态与退出码同 `ky check`。
- 支持 Gmail、163 / 126 / yeah.net、QQ / Foxmail、iCloud，其他域名按 `imap.<域名>:993` 尝试；Outlook 已关闭 IMAP 基本认证，明确报不支持。
- 新增直接依赖 `rustls` / `webpki-roots`，与 `ureq` 共用同一份 ring 实现，不引入第二套 TLS。

### 服务器 Agent 协作：OpenSSH forced-command broker（Experimental）

目标：服务器 Agent 只取当前任务获准的凭据，不拿整库 master key，也不需要把主密码复制到每台机器。

- **`ky agent init` + `ky broker admit`**：服务器侧只生成自己的 key 并打印一张 `kyticket-…`（不含任何凭据）；vault host 侧一条命令验票、按设备 ID 回查 cs 台账、登记精确授权、把 forced-command 行写进 authorized_keys（写前备份、同设备不重复追加）。重复 admit = 续期/改授权，不报错也不堆行。两台机器各一条命令即可接入。
- **`ky broker adopt <host-id>`**：设备身份不再手填，直接从 CortexOS 舰队台账（`registry/fleet-hosts.json`）读 id / public_ip / agent_id；kyvault 只补公钥指纹与逐条凭据授权。台账缺 IP 时拒绝领养（不退化成任意来源）；同一公网 IP 上的其他主机只提示不合并身份（实际台账里 gigi 与 xiaomei 同 IP，按 IP 授权等于按机器授权）。决策记录见 `docs/decisions/2026-10-22-broker-ownership.md`。
- **`ky broker register|list|status|revoke|authorized-key`**：本地管理员审批设备 SSH 公钥、来源 IP/CIDR、精确 ref 与有效期，生成带 `restrict,from=...,command=...` 的 authorized_keys 行供人工审阅安装。
- **`ky broker serve`**：只能作为 sshd fixed command 运行；从 `SSH_CONNECTION` 取实际对端 IP，校验设备状态、CIDR、精确 ref、期限与撤销。
- **精确到叶子**：拒绝 `secret://server/<host>` 这类聚合 ref，避免一次授权读出该主机全部字段；也拒绝 cli 聚合。
- **`ky broker probe|run --expect-device`**：Agent 用自己的 SSH 私钥连接，强制校验 host key、禁用密码/转发/ControlMaster，并核对返回设备 ID；`run` 把值注入本机子进程。
- **强制审计**：allow/deny、请求 ID、设备、来源 IP、ref 列表写入 `broker-audit.jsonl`；审计写失败则拒绝释放凭据。审计不含明文、密文、last4 或客户端原文。

边界（务必如实对待）：`principal`/`--as` 是人工标签；真实身份是安装到 forced command 的 SSH key。IP 是 sshd 看到的对端地址，跳板/NAT 后是跳板出口。撤销只阻止后续请求，已取走的静态值不能追回；本地审计不防同机管理员篡改。部署必须使用专用低权限 SSH 账户并锁定 sshd 配置，程序不会自动修改你的 SSH 设置。Mac/Windows 主设备签名审批、一次性 enrollment 与自动轮换尚未实现。

### 凭据有效性与重复监察（Added）

- **`ky check`** 现在区分 `valid / invalid / permission_limited / rate_limited / unknown / api_error / unsupported`，把状态、HTTP 分类、检查时间与耗时写入 `health.json`（0600，不含值/文案/last4）。429 与 403 不再被当作“已认证”；退出码改为 0 通过/限流、1 认证失败、2 未知，**不再建议任何非零即轮换**。
- **`ky inspect`**：离线只读盘点平台/类型/账户、profile 关联、孤儿引用、同 URI 多存储位置、时间字段异常与历史健康状态；默认不读 master key。
- **`ky inspect --duplicates`**：显式解密后用一次性随机 HMAC 在内存中分组，只输出重复 ref 列表，不输出明文、尾号或指纹。
- `ky panel` 增加设备与历史健康段，并明确这些仍是敏感元信息，`noindex` 不是访问控制。

### 安全模型更正：旧 Auth Guard 不验证 SSH 私钥

此前文档把 Auth Guard 描述成「SSH 公钥绑定保护 master.key」「本机凭 SSH Key 零交互解锁」。代码复核发现：旧实现实际从公开 `.pub` 文件派生 AES key，只检查指纹，不会请求私钥签名；TOTP 校验函数也没有被任何敏感命令调用。拿到 `auth.json`、`master.key.enc` 与公钥即可重算解密 key。旧实现不构成私钥保护、多因素认证或 Agent 身份认证。

- 旧 Auth Guard 的 `setup` / `add` 现在默认拒绝新建或扩展；仅为旧格式迁移兼容时可显式设 `KYVAULT_ALLOW_INSECURE_LEGACY_AUTH=1`。已有数据仍可读、导出或关闭，后续需提供到安全 key wrapping 的迁移工具。
- 新协作模型区分 Local、Git Sync、Broker。按 ref 的 Agent Grant、设备身份、服务端审计与 IP 附加策略见 `docs/proposals/2026-10-22-human-agent-collaboration.md`。
- 安全表述的更正已落实到代码与文档；broker 已实现在上一节（实验性），旧 auth 文件到安全 key wrapping 的迁移/替换流程仍需单独设计。

### 操作审计：谁读过、谁改过（Added）

此前只有 `meta.json` 里的 `updated_at` —— 时间有，人不明；而 `get` 是唯一会输出
明文的命令，读行为却完全不留痕。泄露之后连「有没有人读过」都答不上来。

- **`ky audit`**：本地 `audit.log`（JSONL，0600）记录 `get / set / delete /
  annotate / run / import / pull / push / sync / init / profile` 的时间、actor、
  目标 ref、结果。**不记明文、不记密文、不记 last4** —— 审计日志本身若成了泄漏面，
  就抵消了加它的意义。
- **actor 来源**：`--as <name>` → `KY_ACTOR` → 库内 `git user.name` → `$USER`。
- **团队维度**：`gitlab push` 的提交信息从固定模板改为
  `chore(secrets): sync by <actor> at <ts>`；库的 git 身份也从写死的 `kyvault`
  改为真实操作者。以前所有人看起来都是同一个「kyvault」，等于没有作者信息。
- **默认不入仓**：`audit.log` 含读事件，push 到团队仓就变成监视而不是审计。
  需要共享时设 `KY_AUDIT_SYNC=1`；`KY_AUDIT=0` 可整体关闭。

### 多账户体系：一个 profile = 一个账号（Added）

`account / key / cli / server / identity` 五套平行命令都回答不了那句最常见的问话：
「我有几个 GitHub 账号，每个挂了几条东西，现在用的是哪个？」

- **`ky profile add|list|show|use|current|link|unlink|remove`**，落 `accounts.json`
  （结构性元信息，入仓共享；权限 0600，因为写着别人的登录名）。
- **`ky run --profile <name>`**：把该账号名下所有 ref 一次注入子进程，变量名 =
  ref 名大写化（`secret://github/pat` → `PAT`）。这是「一把钥匙开一批锁」的用法。

### 只读托管面板（Added）

- **`ky panel --out panel.html`**：自包含静态 HTML，含账号、条目、操作计数与最近
  审计。不含明文、密文、last4，带 `noindex`，可直接扔到 GitHub Pages / 对象存储。
- 刻意不提供「把 `ky ui` 挂公网」：`ui` 可写、能看明文，必须只在本机访问。
  （只绑 `127.0.0.1` 不够，见本节顶部的 `ky ui` 漏洞修复。）

### 复用本机 glab 登录态（Added）

以前只认 `KY_GITLAB_TOKEN`，没有就「不代你建仓库」—— 而凭证就在旁边的
`glab-cli/config.yml` 里躺着。

- **凭证优先级**：`KY_GITLAB_TOKEN` → glab 配置文件 → `glab auth status --show-token`。
- **`ky gitlab auth`**：只读探测，打印将使用哪个 host、凭证来自哪里、token 脱敏摘要。
- **建私有库**：`ky init --remote` 在 glab 已登录时直接建项目（`visibility=private`）
  并首推；token 走环境变量交给一次性 credential helper，不落 argv。
- **API 通道**：优先 `glab api`（token 不进 `ps`）；没有 glab 才回落 `curl`，且
  header 走 `--config -` 从 stdin 读。

### 可发现性：让项目能被找到（Added）
起因是一次真实的困惑：在 GitHub 搜「token管理」「秘钥管理」，搜不到这个工具。
查下来发现项目一直被收录着，是**描述和 topics 里根本没有这些词** ——
GitHub 默认搜索索引的是 name + description + topics，README 不参与。
实测：同一句话放在 README 里，`in:readme` 有 710 条结果，默认查询只有 1 条。

### 可发现性 (Added)

- **发布到 crates.io**：`cargo install kyvault` 现在可用（crate 名 `kyvault`，
  `ky` 仍是 install.sh 建的符号链接 —— crates.io 上 `ky` 这个名字已被别的包占用）。
  已实测从 crates.io 真实安装并运行成功。
- **公开站点**：`https://<owner>.github.io/kyvault/`，带 SEO meta、FAQ 结构化数据
  （SoftwareApplication + FAQPage）、明暗双主题与对比度达标的配色。
  `homepage` 字段已从「指向自己」改为指向它。
- **仓库描述改为中英双语**，覆盖实际检索词：密钥管理 / 秘钥管理 / 令牌管理 /
  Secret Manager / Secret Management / API Key Manager / Token Manager /
  Credentials / dotenv alternative。
- **topics 重排为 20 个**：去掉已停用的 `d1`（v2.3.0 起 GitLab 后端替代 D1），
  补齐 `secret-manager` / `secrets-management` / `token-management` /
  `token-manager` / `api-key-manager` / `local-first` 等类目词。
  竞品 Infisical / dotenvx / sops 的共同做法是四种形态全收。
- **徽章**：新增 crates.io 版本徽章与 CI 状态徽章；修正了原本指向
  `#-安装` / `#-install` 的失效锚点（实际标题是「30 秒上手 / 30-second start」）。
- **Homebrew 公式与 winget 清单**已备好并校验（winget 三份清单通过微软官方
  JSON schema 校验；Homebrew 的 sha256 从真实 release 产物算出，未留占位）。

### 表述收紧 (Changed)

README 与站点此前把「AI 永远看不到明文」「全程零网络」当作承诺，这两条都不成立：
`ky get` 会输出明文，GitLab 密文同步与 D1 后端会联网。
现改为按实际行为描述 —— `ky run` 注入时明文不进模型上下文，但 `ky get` 与子进程输出仍会暴露。
同时删掉与实现不符的「静态二进制」（Linux GNU 目标依赖系统库），
以及误挂的 Python 生态徽章（Built with uv / code style Black）。

### 实测效果

搜索排名（同查询、按仓库 id 匹配，改前 → 改后）：
秘钥管理 不存在 → #1 · dotenv alternative 不存在 → #1 · 令牌管理 不存在 → #4 ·
API密钥管理 不存在 → #5 · 密钥管理 #10 → #6 · token管理 不存在 → #12。

## [2.6.0] - 2026-10-19

**长密钥终于能安全写进去了；密文权限不再被 Git 打回 664。**

这一版把 v2.5.0 之后合入 main 的 7 个提交发布出来 —— 它们此前只在 main 上，
`ky update` 拿不到。三个方向：

### 多行 / PEM 密钥（#1 #2）

- **`ky set --file <path>`**：`@` 可省（`--file @/path/to/key.pem` 与 `--file /path/to/key.pem` 都收）。
  与 `value` / stdin 互斥，两个都给直接报错而不是静默挑一个。
  解决的是把 PEM 塞进 `argv` 的老路 —— 明文进 `ps` 与 shell history，README 自己反对的形态。
- **写入时拒收被截断的 PEM**：出现 `-----BEGIN <X>-----` 却没有配对的 `-----END <X>-----` 就拒绝写入，
  提示「看着像被截断了」。PEM 无论怎么写都至少两行，单行 PEM 一定是截断，
  所以不需要用户额外声明「这是故意的」。确认无误加 `--force`。
- **`get` 时校验完整性**：有 sha256 的条目复算比对，不符**报错且不返回明文**
  （明文已经不对了，交给下游只会换一个更难查的报错）。
  存量条目没记 sha256 就照常读出 —— 拿「本来就没记」当「校验失败」会把整库变不可用，比不校验更糟。
- **`meta.json` 记录 `byte_len` / `line_count`**：多行密钥截断时这两个数最直观。
  2026-09-19 那次私钥只剩一行，`line_count=1` 一眼就看出来。

边界说清楚：完整性校验只对 2026-09 之后经 `set` 写入的条目有效，
更早被截断的存量密文它救不回来。

### 密文文件权限（#13）

- **每次打开库前自动收紧**：`Store::repair_perms()` 在 `default_location()` 里调用，
  所有 CLI 路径都经过该函数，改一处即全覆盖。

  根因不是「忘了 chmod」：`save()` / `save_meta()` 两条写入路径本来就都调了 `harden()`。
  真正的来源是**数据目录本身是个 Git 仓库**（团队密钥库模式）—— git 不保存文件权限，
  `checkout` / `pull` 落盘时按当前 umask 建文件（默认 002 → 664），哪怕仓里的 blob 记录的是 600。
  所以每次同步密钥库，密文文件权限就被打回 664；手动 chmod 只能治当前这台机器，下次 sync 又复发。
  这解释了为什么 `master.key.enc` / `auth.json` 一直是 600 —— 它们从没进过 git。

  刻意做成尽力而为：文件不存在就跳过，绝不因为权限问题挡住取密钥 ——
  权限是卫生问题，取不到密钥才是事故。

- **`ky gitlab pull` / `setup` 结束后同样收紧**，覆盖成功、冲突、无效 JSON 与失败克隆路径，保留原始错误。

### CI 与协作

- **仓库级 pre-commit 门禁**（`scripts/pre-commit-all.sh`）：只查 `cargo fmt --check`。
  main 的 CI 从 10-05 起一直红着，卡在 fmt（纯格式漂移，零语义改动），
  把后续两个 PR 全挡住了 —— 包括完全不碰 Rust 的那个，
  因为 fmt 卡的是整个 crate，不管你改没改 Rust。新增 5 条权限回归测试（不访问真实密钥库）。

- **issue 分诊机器人**（#11 #12）：盯本仓 open issue，把「谁说了什么修好了」与
  「代码里到底有没有」对上，结论写成可更新的置顶评论 + 标签。

  只查、只说、只贴标，**不自动关闭**。这条规则是被真实事故逼出来的：
  kyvault#1 的评论里写着「已修」、v2.4.0 也确实发布了，但 `--file` 与 PEM 校验根本没进代码。
  「有人声称修了 + release 存在」这种组合**不能**作为关闭依据。真要关由维护者发 `/triage close #N`。

### 已知未修

- **#5（master.key passphrase 派生 / agent 跨机取密钥）仍未修**。
  现状是 `master.key` 仍需带外传输，这是 agent 场景的阻塞项，属设计级改造。
  #3（GitLab 后端替代 D1）已实现：`KYVAULT_BACKEND` 默认 file，`ky gitlab sync` 可用。

## [2.4.1] - 2026-09-23

**开源纯粹化与体验打磨：确立原生短命令 `ky` 为第一公民，彻底剥离私有上下文。**

- **短命令 `ky` 原生支持**：安装脚本与全局 CLI 全面对齐 `ky`，直接使用 `ky list`、`ky run`、`ky ui`、`ky wizard`。
- **纯粹开源文档治理**：彻底剥离任何私有内部调度系统概念与陈旧历史指引，全面展现 `kyvault` 作为通用独立本地凭证中心的纯粹架构。
- **安装器优化**：`install.sh` 自动建立 `ky` 软链接，默认数据目录对齐 `~/.config/kyvault/store`。

## [2.5.0] - 2026-10-05

**每个密钥库默认有自己的 Git 仓库；上传远端可选。**

- `ky init` 默认给新库建一个本地 Git 仓库并写好 `.gitignore`（不联网、不需要 token）。
  密文进 Git 是安全的 —— AES-256-GCM 密文进了仓也解不开；**master.key /
  master.key.enc / auth.json 由 .gitignore 挡住，永不进历史**。
- `ky init --remote <地址>`：记下上传仓库地址。给了环境变量 `KY_GITLAB_TOKEN`
  才会代你在远端把仓库建出来；没给就只设 origin 并明确说「我不代你建」——
  假装建好是最坏的一种失败：以为有备份，实际 push 直接 404。
- `ky init --no-git`：整个跳过。已有库不会被改动。
- 修：`ky auth add ssh` 记绝对路径。`auth.json` 会被带外复制到另一台机，而那台机
  的 home 不同，记绝对路径会让目标机展开成自己的 home 找不到文件、unlock 静默
  跳过，最后报「没有匹配的 SSH 公钥」。shell 里 `~` 默认先被 bash 展开才传给
  程序，所以在工具这侧归一成 `~/` 形式。

为什么一库一仓而不是共用一个仓：共用意味着 A 和 B 的密文在同一条历史里，
任何一次 push 都把别人的密文带走。一仓一库是「密文可进 Git、根密钥永不入仓」
这套模型的最小完整单元。

## [2.4.0] - 2026-09-22

**旧版 Auth Guard 配置格式（当前仅兼容读取；其安全承诺已撤回）。**

这一版引入了 `auth.json` / `master.key.enc` 包裹流程。后续安全复核发现它从公开 SSH `.pub` 字节派生 AES key，不验证私钥持有；TOTP 校验函数也没有接入敏感命令。因此，下面的 2026-09-22 原始说明记录当时交付的功能形状，不能作为当前安全保证。该格式新建/扩展默认停用；详见本文件 [Unreleased 安全模型更正](#unreleased)。

### 原始交付内容（历史记录）
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
