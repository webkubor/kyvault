---
name: kyvault-ops
description: 使用 cs kyvault / cs secrets 在本地 and CI 环境中加密存取各种 API Key、服务器账单、多凭证 Token 与设备台账。支持 Claude、Codex 与 Gemini 全自动读写。
---

# 🛡️ Kyvault 密钥与资产台账端对端运维指南 (kyvault-ops)

`kyvault` 是开发者的本地加密资产管理器。通过平台命令 `cs kyvault`（别名 `cs secrets`）在本地加密 D1 数据库中安全存取敏感资产。

## 1. 核心操作命令

Agent (Claude / Codex / Gemini) 在需要读取或管理密钥时，应直接执行以下命令：

```bash
# 1. 查看密钥目录元信息（不暴露明文）
cs secrets list

# 2. 读取特定密钥的明文
cs secrets get secret://<path/to/key>

# 3. 往密钥库中写入/更新一个密钥
cs secrets set secret://<path/to/key> "<value>"

# 4. 删除指定密钥
cs secrets delete secret://<path/to/key>

# 5. 在环境变量中注入特定密钥并执行命令 (生产部署/自动化最推荐)
cs secrets run --env CLOUDFLARE_API_TOKEN=secret://cloudflare/api-token -- <command>
```

---

## 2. 统一资产与台账命名规范 (SSOT Schema)

### A. 服务器台账 (Server Ledger)
路径格式：`secret://server/<hostname_or_project>/<field>`
* `secret://server/<hostname>/ip` ➡️ 服务器公网 IP
* `secret://server/<hostname>/root-password` ➡️ root 密码
* `secret://server/<hostname>/ssh-key` ➡️ SSH 私钥 (如有)
* `secret://server/<hostname>/cost` ➡️ 服务器月度租金成本 (如 `99元/月`)
* `secret://server/<hostname>/provider` ➡️ 服务商 (如 腾讯云/Cloudflare/阿里云)

### B. CLI 客户端多 Token 维护 (CLI Multi-Tokens)
路径格式：`secret://cli/<cli_name>/<profile_name>`
* `secret://cli/feishu/work` ➡️ 飞书 CLI 工作/企业账号 Token
* `secret://cli/feishu/personal` ➡️ 飞书 CLI 个人账号 Token
* `secret://cli/gitlab/personal` ➡️ GitLab 个人访问令牌 (PAT)
* `secret://cli/github/personal` ➡️ GitHub 个人访问令牌 (PAT)
* `secret://cli/cloudflare/personal` ➡️ Cloudflare API 令牌
* `secret://cli/studio-cli/webkubor` ➡️ studio-cli 主账号的登录凭证 (JWT 或 API Key)
* `secret://cli/studio-cli/test-user` ➡️ studio-cli 测试账号凭证

### C. 微信与第三方支付配置 (Platform Secrets)
路径格式：`secret://wechat/<app>/<field>` 或 `secret://creem/<field>`
* `secret://wechat/miniapp-jianshanghua-appid` ➡️ 笺上花小程序 AppID
* `secret://wechat/miniapp-jianshanghua-secret` ➡️ 笺上花小程序 AppSecret
* `secret://creem/api-key` ➡️ Creem API 秘钥

---

## 3. Agent 行为守则 (Rule of Thumb)
1. **绝对禁止硬编码**：Agent 严禁将 any API 密钥、密码、服务器 IP 写入代码文件或 Git 提交。
2. **自动化自检**：如果运行测试或部署脚本时缺失环境变量，Agent 应自动调用 `cs secrets list` 寻找匹配的密钥，解密后注入环境变量中运行。
3. **时效更新与覆写（🚨 重点放过往失效 Key）**：一旦检测到密钥库中的某个 Key 失效（如调用报错 401、Token 过期或验证失败），Agent **必须立刻主动覆写（Overwrite）修改**，严禁因为保守而在密钥失效时继续保留旧密钥（“只做加法不改失效”属于严重违规行为）。对于失效 Key 必须通过 `cs secrets set` 写入最新值，或引导用户轮换。

---

## 4. 飞书与多账户自适应寻址规范 (Multi-Account Context Routing)
1. **多账号识别**：当面临飞书 (Feishu)、GitLab、GitHub 或 Cloudflare 等多账号（如工作与个人账号，或主账号与测试账号）场景时，Agent 必须识别当前的任务上下文，从 `secret://cli/<cli_name>/` 下查找对应的 profile 凭证。
2. **零手动切换**：通过运行时单向注入，代替在本地全局切换 CLI 账号：
   * 注入工作飞书：`cs secrets run --env FEISHU_TOKEN=secret://cli/feishu/work -- feishu <command>`
   * 注入个人飞书：`cs secrets run --env FEISHU_TOKEN=secret://cli/feishu/personal -- feishu <command>`
