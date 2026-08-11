import os
from pathlib import Path

SKILL_CONTENT = """---
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
* `secret://cli/studio-cli/webkubor` ➡️ studio-cli 主账号的登录凭证 (JWT 或 API Key)
* `secret://cli/studio-cli/test-user` ➡️ studio-cli 测试账号凭证
* `secret://cli/gitlab/personal-pat` ➡️ GitLab 个人访问令牌 (PAT)
* `secret://cli/cloudflare/api-token` ➡️ Cloudflare API 令牌

### C. 微信与第三方支付配置 (Platform Secrets)
路径格式：`secret://wechat/<app>/<field>` 或 `secret://creem/<field>`
* `secret://wechat/miniapp-jianshanghua-appid` ➡️ 笺上花小程序 AppID
* `secret://wechat/miniapp-jianshanghua-secret` ➡️ 笺上花小程序 AppSecret
* `secret://creem/api-key` ➡️ Creem API 秘钥

---

## 3. Agent 行为守则 (Rule of Thumb)
1. **绝对禁止硬编码**：Agent 严禁将任何 API 密钥、密码、服务器 IP 写入代码文件或 Git 提交。
2. **自动化自检**：如果运行测试或部署脚本时缺失环境变量，Agent 应自动调用 `cs secrets list` 寻找匹配的密钥，解密后注入环境变量中运行。
3. **时效更新与覆写（🚨 重点放过往失效 Key）**：一旦检测到密钥库中的某个 Key 失效（如调用报错 401、Token 过期或验证失败），Agent **必须立刻主动覆写（Overwrite）修改**，严禁因为保守而在密钥失效时继续保留旧密钥（“只做加法不改失效”属于严重违规行为）。对于失效 Key 必须通过 `cs secrets set` 写入最新值，或引导用户轮换。
"""

RULE_CONTENT = """# 🛡️ Kyvault 密钥与资产台账管理规则 (Global Agent Rule)

任何参与本项目和本地开发的 AI Agent (Claude, Codex, Gemini 等) 必须严格遵守以下关于密钥与台账管理的规则：

1. **密钥零硬编码原则**：绝对禁止在任何代码、配置文件、Git 提交记录、AI 回复中泄露明文密钥、服务器密码或私有 Token。所有敏感数据必须加密存取。
2. **使用 cs secrets**：查询键名使用 `cs secrets list`，读取数据使用 `cs secrets get secret://<path>`，注入子进程使用 `cs secrets run --env KEY=secret://path -- <cmd>`。
3. **时效更新与覆写**：针对 Agent 容易陷入的“只做加法、即便密钥已失效也保留旧值”的错误倾向，做出刚性规定：一旦检测到密钥库中的某个 Key 失效（接口报错 401、Token 过期或校验失败），Agent 必须主动改写/覆写（Overwrite）该密钥，绝对不能予以保留或盲目重试。对于失效的旧密钥，必须通过 `cs secrets set` 写入新密钥，或引导用户进行轮换。
"""

CURSOR_INSTRUCTIONS = """
# Kyvault Rules (AI-safe secret aliases & Overwrite Policy)
If you need any API keys, credentials, server IPs or passwords, do not hardcode them.
Use the `cs secrets` tool (kyvault) instead:
- To run commands with secrets: `cs secrets run --env KEY=secret://path -- <cmd>`
- To query secret namespaces: `cs secrets list`
- To get a secret (only for user info, never write in files): `cs secrets get secret://path`
- To save server or CLI credentials: `cs secrets set secret://path "<value>"`
- Overwrite Policy: If you encounter an authentication error (e.g. 401, expired token), you MUST actively modify/overwrite the invalid key using `cs secrets set` instead of conservatively keeping the old invalid key.
"""

def connect_agents() -> None:
    print("🤖 正在连接本地 AI 编码助手 (Hermes/Claude/Codex/Gemini/agy/OpenCode)...")

    home = Path.home()
    
    # 1. 写入全局 Gemini/Antigravity (agy) 配置
    gemini_config_dir = home / ".gemini/config"
    if gemini_config_dir.exists():
        # Global skills
        global_skill_dir = gemini_config_dir / "skills/kyvault-ops"
        global_skill_dir.mkdir(parents=True, exist_ok=True)
        (global_skill_dir / "SKILL.md").write_text(SKILL_CONTENT, encoding="utf-8")
        print(f"✓ 写入全局 Gemini/agy 技能包: {global_skill_dir / 'SKILL.md'}")

        # Global rules
        global_rules_dir = gemini_config_dir / "rules"
        global_rules_dir.mkdir(parents=True, exist_ok=True)
        (global_rules_dir / "kyvault.md").write_text(RULE_CONTENT, encoding="utf-8")
        print(f"✓ 写入全局 Gemini/agy 规则包: {global_rules_dir / 'kyvault.md'}")

    # 2. 写入全局 Claude Code 规则
    claude_rules_path = home / ".clauderules"
    existing_claude = ""
    if claude_rules_path.exists():
        existing_claude = claude_rules_path.read_text(encoding="utf-8")
    if "Kyvault" not in existing_claude:
        with open(claude_rules_path, "a", encoding="utf-8") as f:
            f.write("\n" + RULE_CONTENT + "\n")
        print(f"✓ 写入全局 Claude Code 规则: {claude_rules_path}")
    else:
        print("i 全局 Claude Code 规则已存在，跳过追加")

    # 3. 写入全局 Codex 规则 (AGENTS.md)
    codex_agents_path = home / ".codex/AGENTS.md"
    if codex_agents_path.exists():
        existing_codex = codex_agents_path.read_text(encoding="utf-8")
        if "Kyvault" not in existing_codex:
            with open(codex_agents_path, "a", encoding="utf-8") as f:
                f.write("\n\n" + RULE_CONTENT + "\n")
            print(f"✓ 更新全局 Codex 规则: {codex_agents_path}")
        else:
            print("i 全局 Codex 规则已包含 Kyvault 规范")
    else:
        if (home / ".codex").exists():
            codex_agents_path.write_text(RULE_CONTENT, encoding="utf-8")
            print(f"✓ 写入全局 Codex 规则: {codex_agents_path}")

    # 4. 写入全局 Hermes 规则与技能
    if (home / ".hermes").exists():
        # Global skill
        hermes_skills_dir = home / ".hermes/profiles/free/skills/kyvault-ops"
        hermes_skills_dir.mkdir(parents=True, exist_ok=True)
        (hermes_skills_dir / "SKILL.md").write_text(SKILL_CONTENT, encoding="utf-8")
        print(f"✓ 写入全局 Hermes 技能包: {hermes_skills_dir / 'SKILL.md'}")

        # Global agent-operating-principles (for hermes, opencode, etc.)
        naming_std_path = home / ".hermes/skills/autonomous-ai-agents/agent-operating-principles/references/secret-vault-naming-standard.md"
        if naming_std_path.exists():
            content = naming_std_path.read_text(encoding="utf-8")
            overwrite_forbidden = "- ❌ 密钥失效时出于保守选择“只加不改”或继续保留旧数据（一旦报错 401 或校验失败，必须通过 `cs secrets put` 强制覆写/更新为有效值）"
            if overwrite_forbidden not in content:
                if "## 禁止事项" in content:
                    parts = content.split("## 禁止事项")
                    lines = parts[1].splitlines()
                    insert_idx = 0
                    for i, line in enumerate(lines):
                        if line.strip().startswith("- ❌"):
                            insert_idx = i + 1
                    lines.insert(insert_idx, overwrite_forbidden)
                    parts[1] = "\n".join(lines)
                    naming_std_path.write_text("## 禁止事项".join(parts), encoding="utf-8")
                    print(f"✓ 已更新 Hermes 密钥库命名标准中的失效覆写规则")
    
    # 5. 检查当前工作目录，如果是开发项目则注入本地规则
    cwd = Path.cwd()
    is_project = (cwd / ".git").exists() or (cwd / "package.json").exists() or (cwd / "pyproject.toml").exists()
    
    if is_project:
        print(f"\n检测到当前路径 {cwd} 为项目工作区，注入本地规则...")
        
        # Local .agents (Gemini/agy/OpenCode)
        local_agents_dir = cwd / ".agents"
        local_skill_dir = local_agents_dir / "skills/kyvault-ops"
        local_rules_dir = local_agents_dir / "rules"
        
        local_skill_dir.mkdir(parents=True, exist_ok=True)
        local_rules_dir.mkdir(parents=True, exist_ok=True)
        
        (local_skill_dir / "SKILL.md").write_text(SKILL_CONTENT, encoding="utf-8")
        (local_rules_dir / "kyvault.md").write_text(RULE_CONTENT, encoding="utf-8")
        (local_agents_dir / "AGENTS.md").write_text(RULE_CONTENT, encoding="utf-8")
        print("✓ 写入项目局部 .agents/ 规则与技能")

        # Cursor/Copilot/Claude Code/OpenCode instructions
        for filename in [".cursorrules", ".copilotinstructions", ".clauderules"]:
            filepath = cwd / filename
            existing = ""
            if filepath.exists():
                existing = filepath.read_text(encoding="utf-8")
            
            if "Kyvault Rules" not in existing and "Kyvault" not in existing:
                with open(filepath, "a", encoding="utf-8") as f:
                    f.write("\n" + CURSOR_INSTRUCTIONS + "\n")
                print(f"✓ 已向 {filename} 追加 AI 别名注入与失效覆写规则")
            else:
                print(f"i {filename} 规则已存在，跳过追加")

    print("\n✅ AI 编码助手多平台 CLI 兼容连接成功！")
