# Broker 上机步骤（单台服务器接入）

> 适用版本：工作区当前未发布改动。先在一台**非生产**服务器上走完，再推广到其他机器。
> 这套流程不改你现有的 SSH 配置，需要你手工装一行 authorized_keys。

## 0. 先决条件

**先选部署形态**（这一步决定后面所有步骤）：

| 形态 | 说明 | 代价 |
|---|---|---|
| A. Mac 直接当 broker | 库本来就在 Mac | Mac 必须开 sshd 并对服务器入站开放；家用宽带 NAT/动态 IP 下基本不可行，公网暴露 sshd 也不划算 |
| B. 专用/加固机器持库（推荐） | 库与 broker 一起放在一台长期在线的机器（可以是舰队里某台加固机，或新开一台） | 这台机持有 master key，必须单独加固；Mac 变成**审批端**（离线签发授权，不常驻） |
| C. 反向隧道 | 服务器主动连回 Mac | 仍需 Mac 可入站，复杂度更高，v1 不做 |

下面按 **B** 写。若你只有 Mac 一台机，先看清楚 A 的代价再决定。

- **vault host**（形态 B 的那台）：已 `ky init` 过，库目录只有服务账号可读，且它能被 agent host 主动连上（22 端口可达或走跳板）。
- **agent host**（要取凭据的服务器）：有 `kyvault` 二进制（同版本），**不需要** master key，**不需要** secrets.json。
- 专用 SSH 账户（建议 `kybroker`）：只用于 broker，不给普通登录/部署用。若沿用你自己的账号，必须确认该账号没有其他可登录的 key。

先自查（任一条不满足就别往下走）：

```bash
# vault host 上
ls -ld ~/.config/kyvault/store          # 期望 drwx------（700）
ls -l  ~/.config/kyvault/store/master.key  # 期望 -rw-------（600）
which kyvault && kyvault --version

# agent host 上
which kyvault
kyvault broker --help >/dev/null && echo "同版本 OK"
```

## 0.5 先把 ky 装到所有 Agent 机器

原则：**每台跑 Agent 的服务器都要有 `kyvault`**（只需要二进制，不需要 master key、不需要 secrets.json）。没有它，Agent 无法在本地注入，只能拿明文。

```bash
# 在每台 agent host 上（有网时可直接用官方安装器）
curl -fsSL https://raw.githubusercontent.com/webkubor/kyvault/main/install.sh | bash

# 内网 / 无外网：从一台机 scp 过去
scp kyvault-linux-amd64 <host>:~/.local/bin/kyvault
ssh <host> 'chmod +x ~/.local/bin/kyvault && ln -sf ~/.local/bin/kyvault ~/.local/bin/ky && ky --version'
```

要点：

- **版本要一致**。broker 的请求/响应格式带 `version`，客户端与服务端版本不一致会直接拒绝，不会静默降级。
- **glibc 下限**：Linux 二进制在 Debian 12 工具链构建，需要 glibc ≥ 2.36；Ubuntu 22.04/24.04 可用，OpenCloudOS 9（glibc 2.34）不行，需要 musl 静态版。
- 只装二进制**不会**让服务器获得任何凭据——没有库、没有 key，它就是客户端。
- 装完用 `ky --version` 和 `ky broker --help >/dev/null` 两句话验收，前者看版本，后者看这个版本到底有没有 broker 子命令。

## 1. 两台机器各一条命令

**服务器上**（不接触任何凭据，只生成自己的 key）：

```bash
kyvault agent init --id vex
```

它打印一张 `kyticket-…` 和一串下一步命令。把票复制走。

**vault host 上**（身份从 cs 台账自动带出，不用手填 IP）：

```bash
kyvault broker admit 'kyticket-…' \
  --allow-ref secret://github/deploy \
  --ttl-hours 24 \
  --install ~kybroker/.ssh/authorized_keys
```

`admit` 做了四件事：验票 → 按设备 ID 查 cs 台账（`fleet-hosts.json`）核对这台机确实登记过 → 登记授权（IP 从台账 `public_ip` 取，收敛 `/32`）→ 把 forced-command 行写进 authorized_keys（写前备份，同设备不重复追加）。

- 重复 `admit` = 续期/改授权，不会报错也不会堆出第二行；要停用请显式 `revoke`。
- 每台设备一把独立 key；复用同一把会被拒。
- 台账缺 `public_ip` 时拒绝批准，不退化成「任意来源」。
- 每条 `--allow-ref` 必须是具体条目；`secret://server/<host>` 这类聚合会被拒。
- 同 IP 的其他机器只提示不合并身份：**IP 是门禁，身份是 key 指纹**（你台账里 gigi 与 xiaomei 就是同一台机两个 agent）。

装完确认 sshd 生效：`sudo sshd -t && sudo systemctl reload ssh`。

## 2. 在 vault host 上装 forced command

> 注意方向：**私钥在 agent host**，**authorized_keys 在 vault host**。Agent 是连进来的那一端，`broker serve` 跑在 vault host 上。

把第 1 步输出的那一行（形如 `restrict,from="…",command="… broker serve …" ssh-ed25519 AAAA… kyvault:nanzhu`）追加到 vault host 上专用账户的 `~/.ssh/authorized_keys`：

```bash
# 在 vault host 上执行
cat >> /home/kybroker/.ssh/authorized_keys <<'EOF'
<粘贴那一行>
EOF
chmod 600 /home/kybroker/.ssh/authorized_keys
chown kybroker:kybroker /home/kybroker/.ssh/authorized_keys
```

sshd 侧必须满足（写进 `/etc/ssh/sshd_config` 的 `Match User kybroker` 段，然后 `sshd -t && systemctl reload sshd`）：

```
Match User kybroker
    PermitTTY no
    AllowTcpForwarding no
    AllowAgentForwarding no
    X11Forwarding no
    PermitTunnel no
    PermitUserEnvironment no
    ForceCommand /usr/local/bin/kyvault broker serve --store <绝对路径> --device nanzhu
    AllowUsers kybroker
```

- **不要**在全局或 Match 里设置会覆盖 `command=` 的 `ForceCommand internal-sftp` 之类；最终生效配置以 `sshd -T -C user=kybroker,host=x,addr=203.0.113.10` 为准。
- 专用账户的登录 shell 启动文件（`.bashrc`/`.zshenv`）、`~/.ssh/rc`、`/etc/ssh/sshrc` 必须由你控制；`restrict` 只能挡掉一部分。
- store 目录与 `kyvault` 二进制不能被该账户写入。
- **权限取舍**：`kybroker` 必须能读 store（master.key + secrets.json）才能解密，所以最简单是让 vault 主账号就是 `kybroker`；若拆成两个账号，把 store 归到一个组、`0640/0750`，并确认 `kybroker` 不在可写组里。这是「让 broker 持有钥匙」的必然代价 —— 别在同一目录里再放别的东西。

## 3. 在 agent host 上验证

写一份只给它用的 SSH config（不放你的默认身份）：

```
Host vault-broker
    HostName <vault host 域名或 IP>
    User kybroker
    IdentityFile ~/.ssh/kyvault-agent
    IdentitiesOnly yes
    UserKnownHostsFile ~/.ssh/known_hosts_broker
```

首次连接要人工核对 host key 指纹（`ssh-keygen -lf` 对照 vault host 上 `/etc/ssh/ssh_host_ed25519_key.pub`），确认后写进独立的 known_hosts。

```bash
kyvault broker probe --host vault-broker --expect-device nanzhu \
  --ssh-config ~/.ssh/config-broker

kyvault broker run --host vault-broker --expect-device nanzhu \
  --ssh-config ~/.ssh/config-broker \
  --env GITHUB_TOKEN=secret://github/deploy -- gh auth status
```

`probe` 通过只说明身份、IP、期限、ref 都过；`run` 成功才说明子进程拿到了值。

## 4. 日常运维

```bash
kyvault broker status          # 设备状态、最近成功请求、最近事件（无凭据值）
kyvault broker revoke nanzhu  # 立即拒绝后续请求
kyvault broker list
```

- 撤销**不回收**已经取走的静态凭据。怀疑泄漏就走平台的令牌轮换，然后重新登记。
- 到期前重新 `register`（同一设备 ID 需先 `revoke`）。
- 审计在 `broker-audit.jsonl`，服务端写失败会拒绝发放；但它防不了能登录 vault host 的管理员改写，别把它当不可抵赖账本。

## 5. 验收清单（每台机器都过一遍）

- [ ] 只有专用 key 能登录专用账户（`ssh -o PreferredAuthentications=publickey` 用别的 key 失败）
- [ ] 不带 `--ssh-config` 的默认 `ssh vault-broker` 不能拿到值（应被 forced command 拒绝）
- [ ] 错误 IP 请求被拒（从另一台机器试）
- [ ] 未授权 ref 被拒，且错误信息不含任何密钥值
- [ ] `revoke` 后新请求被拒
- [ ] agent host 上搜不到 `master.key` / `secrets.json`
- [ ] `sshd -T` 里没有覆盖 `command=` 的 ForceCommand

## 6. 已部署实例（2026-10-22）

| 项 | 值 |
|---|---|
| vault host | `vex` = `150.109.17.122` |
| broker 账户 | `kybroker`（shell `/bin/bash`，仅 forced command 可用） |
| 库位置 | `/home/kybroker/.config/kyvault/store`（700，文件 600，290 条） |
| 二进制 | `/usr/local/bin/kyvault`（+ `/usr/local/bin/ky`） |
| sshd | `Match User kybroker`：PermitTTY/转发/隧道/userRC 全关，`MaxSessions 1`；备份 `sshd_config.bak-*` |
| 设备 | nanzhu / guqiuyue / gigi / xiaomei / hym-wecom-relay，各 1 条 `secret://broker/selftest` 授权，24h |
| agent 侧 | `~/.kyvault/agent/agent`（xiaomei 用 `~/.kyvault/agent/xiaomei`）+ `~/.ssh/config-kybroker` |

验证结果：6 台 `broker run` 全部注入成功；冒名（`--expect-device` 不符）与越权 ref 均被拒；审计 8 条 inject / 1 条 probe / 1 条 deny，不含凭据值。

注意：`broker admit` 的 `--allow-ref` 目前都是 `secret://broker/selftest`（自检用），真实凭据授权待按用途逐条加。
