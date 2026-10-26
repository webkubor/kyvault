# 决策记录：broker 归属与设备身份真源

> 日期：2026-10-22 · 决策方式：`cs think`（ssot / separation / worse-is-better）+ `cs rule agent-identity` + `cs server list`
> 状态：已落地（`ky broker adopt`）

## 结论（三句）

1. **broker 留在 kyvault，不进 cs。** `cs rule` 明确「cs 只负责引导，密钥的事全归 kyvault」；加解密、ref 解析、根密钥驻留都是 kyvault 的职责。cs 只做调用方（`cs kyvault` 透传）。
2. **设备身份的真源是 cs 台账，不是 kyvault。** `registry/fleet-hosts.json` 已维护 id / public_ip / ssh_alias / agent_id；kyvault 再存一份必然漂移（ssot 透镜否掉）。
3. **IP 不能当身份，只能当网络门禁。** 实际台账里 `gigi` 与 `xiaomei` 共用 `43.133.71.10`；按 IP 授权等于按机器授权，同机两个 agent 无法区分。

## 落地形态

- `ky broker adopt <host-id> --pubkey ... --allow-ref ... ` 从台账读身份：`principal` 取 `agent_id`，来源 IP 取 `public_ip` 并收敛为 `/32`。
- 台账缺 `public_ip` 时**拒绝领养**，不退化成「任意来源」。
- 同 IP 的其他主机只**提示**，不合并身份；设备区分靠 SSH key 指纹（每台一把，登记时去重，复用直接报错）。
- kyvault 只额外存两样台账里没有的东西：公钥指纹、逐条 ref 授权 + 期限/撤销。

## 被否决的方案及原因

| 方案 | 否决理由 |
|---|---|
| cs/CortexOS 当 broker（持 root key 发凭据） | 与既有边界冲突；cs 一旦持钥，`cs kyvault` 的「不存不解密」承诺立刻失效，且 cs 是 Go 单体，扩一个密钥面等于给控制面加最危险的依赖 |
| kyvault 自建设备表 + 手填 IP | 与 fleet-hosts.json 重复；手填台账必然陈旧（`cs server` 文档原话） |
| 只用 IP 白名单授权 | gigi/xiaomei 同 IP 反例；NAT/跳板下更不可靠 |
| 把 master key 发到每台服务器 | 一次失陷即全库；撤销只能轮换整库（second-order 透镜） |

## 仍未闭合

- 主设备（Mac/Windows）签名审批：当前 `--as` 只是标签，`cs rule agent-identity` 的 administrator 也不构成密码学签名。
- `ssh_key_ref` 字段在台账里已有（如 `secret://cortexos/agent-shared-ssh-private-key`），但共享私钥与「每机一把」冲突，需要单独收敛；未在本次改动。
