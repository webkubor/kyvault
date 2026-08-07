"""从 .env 文件导入密钥到 keyring。"""

import os
import re
from pathlib import Path
from typing import Optional

from .store import set_secret
from .alias import set_alias


def _parse_env_file(filepath: str) -> dict[str, str]:
    """解析 .env 文件。"""
    result = {}
    path = Path(filepath)
    if not path.exists():
        return result

    with open(path) as f:
        for line in f:
            line = line.strip()
            # 跳过空行和注释
            if not line or line.startswith("#"):
                continue
            # 匹配 KEY=VALUE
            match = re.match(r'^([A-Za-z_][A-Za-z0-9_]*)=(.*)', line)
            if match:
                key = match.group(1)
                value = match.group(2).strip()
                # 移除引号
                if len(value) >= 2 and value[0] == value[-1] and value[0] in ('"', "'"):
                    value = value[1:-1]
                result[key] = value
    return result


def _guess_platform(env_key: str) -> str:
    """从环境变量名猜测平台。"""
    key_lower = env_key.lower()
    if "github" in key_lower:
        return "github"
    if "gitlab" in key_lower:
        return "gitlab"
    if "cloudflare" in key_lower or "cf_" in key_lower:
        return "cloudflare"
    if "deepseek" in key_lower:
        return "deepseek"
    if "zhipu" in key_lower or "chatglm" in key_lower:
        return "zhipu"
    if "volcengine" in key_lower or "ark" in key_lower:
        return "volcengine"
    if "feishu" in key_lower or "lark" in key_lower:
        return "feishu"
    if "jenkins" in key_lower:
        return "jenkins"
    if "aws" in key_lower:
        return "aws"
    if "gcp" in key_lower or "google" in key_lower:
        return "gcp"
    if "azure" in key_lower:
        return "azure"
    if "openai" in key_lower:
        return "openai"
    if "anthropic" in key_lower or "claude" in key_lower:
        return "anthropic"
    return "custom"


def _guess_kind(env_key: str) -> str:
    """从环境变量名猜测密钥类型。"""
    key_lower = env_key.lower()
    if "token" in key_lower or "pat" in key_lower:
        return "Token"
    if "key" in key_lower:
        return "API Key"
    if "secret" in key_lower:
        return "Secret"
    if "password" in key_lower or "passwd" in key_lower:
        return "Password"
    return "API Key"


def _make_name(env_key: str, platform: str) -> str:
    """从环境变量名生成密钥名称。"""
    # 移除平台前缀
    name = env_key.lower()
    for prefix in [f"{platform}_", "app_", "service_"]:
        if name.startswith(prefix):
            name = name[len(prefix):]
            break
    # 替换特殊字符
    name = re.sub(r'[^a-z0-9]', '-', name)
    name = re.sub(r'-+', '-', name).strip('-')
    return name or "default"


def import_env_file(
    filepath: str,
    prefix: str = "",
    dry_run: bool = False,
    auto_alias: bool = True,
) -> list[dict]:
    """从 .env 文件导入密钥。

    Args:
        filepath: .env 文件路径
        prefix: 只导入以此开头的环境变量（如 GITHUB_）
        dry_run: 只显示会导入什么，不实际导入
        auto_alias: 自动创建别名

    Returns:
        导入的密钥列表
    """
    env_vars = _parse_env_file(filepath)
    if not env_vars:
        print(f"未找到环境变量：{filepath}")
        return []

    imported = []

    for env_key, value in env_vars.items():
        # 过滤前缀
        if prefix and not env_key.startswith(prefix):
            continue

        # 跳过空值
        if not value:
            continue

        platform = _guess_platform(env_key)
        name = _make_name(env_key, platform)
        kind = _guess_kind(env_key)
        ref = f"secret://{platform}/{name}"

        if dry_run:
            print(f"  {env_key} → {ref} [{kind}]")
            imported.append({"env_key": env_key, "ref": ref, "kind": kind})
            continue

        # 导入
        set_secret(ref, value, kind=kind)
        print(f"✓ {env_key} → {ref}")

        # 创建别名
        if auto_alias:
            alias = env_key.lower()
            set_alias(alias, ref)

        imported.append({"env_key": env_key, "ref": ref, "kind": kind})

    return imported
