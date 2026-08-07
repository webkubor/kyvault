"""交互式向导 — 问几个问题就搞定。"""

import os
import sys
from pathlib import Path

from .store import init_master_key, set_secret, parse_ref
from .alias import set_alias


def _input(prompt: str, default: str = "") -> str:
    """带默认值的输入。"""
    if default:
        result = input(f"{prompt} [{default}]: ").strip()
        return result if result else default
    return input(f"{prompt}: ").strip()


def _confirm(prompt: str) -> bool:
    """确认提示。"""
    return input(f"{prompt} (y/N): ").strip().lower() in ("y", "yes")


def wizard() -> None:
    """交互式设置向导。"""
    print("🔐 Keyring — 设置向导")
    print("=" * 40)
    print()

    # 1. 初始化 master key
    master_key = init_master_key()
    print("✓ Master key 已就绪")
    print()

    # 2. 询问要存储的密钥
    print("接下来设置你的密钥。")
    print("（输入 q 结束设置）")
    print()

    secrets_added = []

    while True:
        print(f"--- 密钥 #{len(secrets_added) + 1} ---")
        platform = _input("平台（如 github, deepseek, zhipu）")
        if platform.lower() == "q":
            break

        name = _input("名称（如 my-pat, api-key）")
        if name.lower() == "q":
            break

        value = _input("密钥值")
        if value.lower() == "q":
            break

        kind = _input("类型", "API Key")
        account = _input("账号（可选，回车跳过）", "")

        ref = f"secret://{platform}/{name}"
        set_secret(ref, value, kind=kind, account=account)
        print(f"✓ 已保存 {ref}")

        # 询问是否创建别名
        default_alias = f"{platform}_token" if "token" in name.lower() or "pat" in name.lower() else f"{platform}_{name}"
        alias = _input(f"创建别名（直接回车用 {default_alias}，输入 n 跳过）", default_alias)
        if alias.lower() != "n" and alias:
            set_alias(alias, ref)
            print(f"✓ 别名 {alias} → {ref}")

        secrets_added.append(ref)
        print()

    # 3. 总结
    print()
    print("=" * 40)
    if secrets_added:
        print(f"✅ 已添加 {len(secrets_added)} 个密钥")
        print()
        print("使用方式：")
        print("  keyring list          # 查看所有密钥")
        print("  keyring get 别名      # 读取密钥")
        print("  keyring run --env X=别名 -- cmd  # AI 用")
    else:
        print("未添加任何密钥。")
        print("稍后可以用 keyring set secret://... 添加")

    print()
    print("更多命令：keyring --help")
