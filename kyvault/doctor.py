import os
import sys
import stat
from pathlib import Path
from .store import SECRETS_DIR, MASTER_KEY_FILE, SECRETS_FILE

def check_permissions(path: Path) -> str:
    """获取文件权限的可读字符串，如 0o600"""
    try:
        return oct(stat.S_IMODE(os.stat(path).st_mode))
    except Exception:
        return "N/A"

def run_doctor() -> None:
    print("🏥 开始运行 Kyvault 工具自检 (Tool Self-Check)...")
    print("-" * 50)

    # 1. 运行环境自检
    print("⚙️  1. 运行环境：")
    print(f"  - Python 版本：{sys.version.split()[0]} ({'OK' if sys.version_info >= (3, 10) else 'FAIL, 推荐 >= 3.10'})")
    print(f"  - 操作系统平台：{sys.platform}")
    try:
        import cryptography
        print(f"  - Cryptography 依赖：v{cryptography.__version__} (OK)")
    except ImportError:
        print("  - Cryptography 依赖：未安装 (FAIL)")

    # 2. 文件与权限自检
    print("\n📁  2. 文件与安全权限：")
    print(f"  - 数据目录：{SECRETS_DIR} ({'存在' if SECRETS_DIR.exists() else '未初始化'})")
    
    # 检查 master.key
    if MASTER_KEY_FILE.exists():
        perms = check_permissions(MASTER_KEY_FILE)
        is_safe = perms in ("0o600", "0o400")
        if not is_safe:
            try:
                MASTER_KEY_FILE.chmod(0o600)
                perm_desc = f"权限：{perms} (⚠️ 权限过高，已自动修复为 600)"
            except OSError:
                perm_desc = f"权限：{perms} (⚠️ 权限过高，建议设置为 600)"
        else:
            perm_desc = f"权限：{perms} (安全)"
        print(f"  - 主密钥文件 (master.key)：存在, {perm_desc}")
    else:
        print("  - 主密钥文件 (master.key)：缺失 (FAIL, 请执行 kyi 初始化)")

    # 检查 secrets.json
    if SECRETS_FILE.exists():
        perms = check_permissions(SECRETS_FILE)
        is_safe = perms in ("0o600", "0o400")
        if not is_safe:
            try:
                SECRETS_FILE.chmod(0o600)
                perm_desc = f"权限：{perms} (⚠️ 权限过高，已自动修复为 600)"
            except OSError:
                perm_desc = f"权限：{perms} (⚠️ 权限过高，建议设置为 600)"
        else:
            perm_desc = f"权限：{perms} (安全)"
        print(f"  - 密钥存储文件 (secrets.json)：存在, {perm_desc}")
    else:
        print("  - 密钥存储文件 (secrets.json)：不存在 (空密钥库)")

    # 3. 密钥状态自检
    print("\n🔑  3. 密钥库解密测试：")
    from .store import get_master_key, _load_secrets
    if not MASTER_KEY_FILE.exists():
        print("  - 解密状态：未初始化，跳过测试")
    else:
        try:
            key = get_master_key()
            secrets = _load_secrets()
            # 尝试解密一个密钥，验证 master.key 是否真实匹配 secrets.json 的加密密钥
            if secrets:
                for platform in secrets:
                    if not platform.startswith("_"):
                        keys_dict = secrets[platform].get("keys", {})
                        if keys_dict:
                            first_key = list(keys_dict.keys())[0]
                            from .store import get_key
                            get_key(platform, first_key)
                            break
            print(f"  - 解密状态：成功 (OK)")
            print(f"  - 包含平台总数：{len(secrets)} 个")
        except Exception as e:
            print(f"  - 解密状态：失败, {str(e)} (FAIL)")

    # 4. 全局 AI 智能体兼容自检
    print("\n🤖  4. 全局 AI 编码助手对接状态：")
    home = Path.home()
    
    # Gemini
    gemini_rule = home / ".gemini/config/rules/kyvault.md"
    gemini_skill = home / ".gemini/config/skills/kyvault-ops/SKILL.md"
    print(f"  - Gemini/agy 规则：{'已连接 (OK)' if gemini_rule.exists() else '未连接'}")
    print(f"  - Gemini/agy 技能：{'已连接 (OK)' if gemini_skill.exists() else '未连接'}")

    # Claude Code
    claude_rule = home / ".clauderules"
    print(f"  - Claude Code 规则：{'已连接 (OK)' if claude_rule.exists() else '未连接'}")

    # Codex
    codex_rule = home / ".codex/AGENTS.md"
    print(f"  - Codex 规则 (AGENTS.md)：{'已连接 (OK)' if codex_rule.exists() else '未连接'}")

    # Hermes
    hermes_skill = home / ".hermes/profiles/free/skills/kyvault-ops/SKILL.md"
    print(f"  - Hermes 技能包：{'已连接 (OK)' if hermes_skill.exists() else '未连接'}")

    print("\n💡 提示：若任何 AI 助手状态为'未连接'，直接运行 'kyvault connect' 即可一键修复。")
    print("-" * 50)
