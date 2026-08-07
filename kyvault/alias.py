"""别名管理 — 人设好映射，AI 只用别名。"""

import json
from pathlib import Path
from typing import Optional

ALIASES_FILE = Path.home() / ".keyring" / "aliases.json"


def _load_aliases() -> dict:
    if not ALIASES_FILE.exists():
        return {}
    try:
        with open(ALIASES_FILE) as f:
            return json.load(f)
    except (json.JSONDecodeError, IOError):
        return {}


def _save_aliases(data: dict) -> None:
    ALIASES_FILE.parent.mkdir(parents=True, exist_ok=True)
    with open(ALIASES_FILE, "w") as f:
        json.dump(data, f, indent=2)


def set_alias(name: str, ref: str) -> None:
    """设置别名：github_token → secret://github/my-pat"""
    aliases = _load_aliases()
    aliases[name] = ref
    _save_aliases(aliases)


def get_alias(name: str) -> Optional[str]:
    """获取别名对应的 secret 引用。"""
    return _load_aliases().get(name)


def resolve_ref(ref_or_alias: str) -> str:
    """解析引用：如果是别名则转换为 secret:// 格式。"""
    if ref_or_alias.startswith("secret://"):
        return ref_or_alias
    alias = get_alias(ref_or_alias)
    if alias:
        return alias
    return ref_or_alias


def list_aliases() -> dict:
    """列出所有别名。"""
    return _load_aliases()


def delete_alias(name: str) -> bool:
    """删除别名。"""
    aliases = _load_aliases()
    if name in aliases:
        del aliases[name]
        _save_aliases(aliases)
        return True
    return False
