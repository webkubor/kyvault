"""本地密钥存储 — 加密存 ~/.keyring/secrets.json，零网络依赖。"""

import base64
import json
import os
from pathlib import Path
from typing import Optional

from .crypto import encrypt, decrypt

SECRETS_DIR = Path.home() / ".keyring"
SECRETS_FILE = SECRETS_DIR / "secrets.json"
MASTER_KEY_FILE = SECRETS_DIR / "master.key"


def _load_secrets() -> dict:
    if not SECRETS_FILE.exists():
        return {}
    try:
        with open(SECRETS_FILE) as f:
            return json.load(f)
    except (json.JSONDecodeError, IOError):
        return {}


def _save_secrets(data: dict) -> None:
    SECRETS_DIR.mkdir(parents=True, exist_ok=True)
    with open(SECRETS_FILE, "w") as f:
        json.dump(data, f, indent=2)


def get_master_key() -> str:
    """获取 master key，优先环境变量，其次本地文件。"""
    key = os.environ.get("KEYRING_MASTER_KEY", "")
    if key:
        return key
    if MASTER_KEY_FILE.exists():
        return MASTER_KEY_FILE.read_text().strip()
    raise SystemExit("未初始化。运行 keyring init 生成 master key。")


def init_master_key() -> str:
    """生成 master key 并保存到本地。"""
    if MASTER_KEY_FILE.exists():
        return MASTER_KEY_FILE.read_text().strip()
    key = base64.b64encode(os.urandom(32)).decode("ascii")
    SECRETS_DIR.mkdir(parents=True, exist_ok=True)
    MASTER_KEY_FILE.write_text(key)
    MASTER_KEY_FILE.chmod(0o600)
    return key


def parse_ref(ref: str) -> tuple[str, str]:
    """解析 secret://platform/name 引用。"""
    if not ref.startswith("secret://"):
        raise ValueError(f"格式错误，应为 secret://platform/name: {ref}")
    parts = ref[len("secret://"):].split("/", 1)
    if len(parts) != 2:
        raise ValueError(f"格式错误：{ref}")
    return parts[0], parts[1]


def _ensure_platform(secrets: dict, platform: str) -> None:
    """确保平台结构存在。"""
    if platform not in secrets:
        secrets[platform] = {"accounts": {}, "keys": {}}
    elif "accounts" not in secrets[platform]:
        secrets[platform]["accounts"] = {}
    elif "keys" not in secrets[platform]:
        secrets[platform]["keys"] = {}


# ── 账户操作 ──────────────────────────────────────────────

def set_account(platform: str, username: str, password: str) -> None:
    """加密并保存账户密码。"""
    ciphertext = encrypt(password, get_master_key())
    secrets = _load_secrets()
    _ensure_platform(secrets, platform)
    secrets[platform]["accounts"][username] = ciphertext
    _save_secrets(secrets)


def get_account(platform: str, username: str) -> Optional[str]:
    """读取并解密账户密码。"""
    secrets = _load_secrets()
    if platform not in secrets:
        return None
    ciphertext = secrets[platform].get("accounts", {}).get(username)
    if ciphertext is None:
        return None
    return decrypt(ciphertext, get_master_key())


def list_accounts(platform: str) -> list[str]:
    """列出平台下所有账户用户名。"""
    secrets = _load_secrets()
    if platform not in secrets:
        return []
    return list(secrets[platform].get("accounts", {}).keys())


def delete_account(platform: str, username: str) -> bool:
    """删除账户。"""
    secrets = _load_secrets()
    if platform not in secrets:
        return False
    accounts = secrets[platform].get("accounts", {})
    if username in accounts:
        del accounts[username]
        _save_secrets(secrets)
        return True
    return False


# ── 密钥操作 ──────────────────────────────────────────────

def set_key(platform: str, key_name: str, value: str) -> None:
    """加密并保存平台密钥。"""
    ciphertext = encrypt(value, get_master_key())
    secrets = _load_secrets()
    _ensure_platform(secrets, platform)
    secrets[platform]["keys"][key_name] = ciphertext
    _save_secrets(secrets)


def get_key(platform: str, key_name: str) -> Optional[str]:
    """读取并解密平台密钥。"""
    secrets = _load_secrets()
    if platform not in secrets:
        return None
    ciphertext = secrets[platform].get("keys", {}).get(key_name)
    if ciphertext is None:
        return None
    return decrypt(ciphertext, get_master_key())


def list_keys(platform: str) -> list[str]:
    """列出平台下所有密钥名。"""
    secrets = _load_secrets()
    if platform not in secrets:
        return []
    return list(secrets[platform].get("keys", {}).keys())


def delete_key(platform: str, key_name: str) -> bool:
    """删除密钥。"""
    secrets = _load_secrets()
    if platform not in secrets:
        return False
    keys = secrets[platform].get("keys", {})
    if key_name in keys:
        del keys[key_name]
        _save_secrets(secrets)
        return True
    return False


# ── 平台查询 ──────────────────────────────────────────────

def list_all_platforms() -> dict[str, dict]:
    """列出所有平台及其账户/密钥摘要。"""
    secrets = _load_secrets()
    result = {}
    for platform, data in secrets.items():
        accounts = list(data.get("accounts", {}).keys())
        keys = list(data.get("keys", {}).keys())
        result[platform] = {"accounts": accounts, "keys": keys}
    return result


def list_platform_detail(platform: str) -> Optional[dict]:
    """列出指定平台的账户和密钥详情。"""
    secrets = _load_secrets()
    if platform not in secrets:
        return None
    data = secrets[platform]
    return {
        "accounts": list(data.get("accounts", {}).keys()),
        "keys": list(data.get("keys", {}).keys()),
    }


# ── 旧接口兼容（alias / import 用） ──────────────────────

def get_secret(ref: str) -> Optional[str]:
    """读取并解密本地密钥（兼容旧 secret:// 格式）。"""
    platform, name = parse_ref(ref)
    secrets = _load_secrets()

    # 新格式：platform/name 在 keys 或 accounts 中
    if platform in secrets:
        plaintext = secrets[platform].get("keys", {}).get(name)
        if plaintext:
            return decrypt(plaintext, get_master_key())
        plaintext = secrets[platform].get("accounts", {}).get(name)
        if plaintext:
            return decrypt(plaintext, get_master_key())

    # 旧格式兼容：platform/name 作为扁平 key
    key = f"{platform}/{name}"
    if key in secrets:
        return decrypt(secrets[key]["ciphertext"], get_master_key())

    return None


def set_secret(ref: str, value: str, kind: str = "API Key", account: str = "") -> None:
    """加密并保存密钥（兼容旧 secret:// 格式，存入 keys）。"""
    platform, name = parse_ref(ref)
    set_key(platform, name, value)


def delete_secret(ref: str) -> bool:
    """删除密钥（兼容旧格式）。"""
    platform, name = parse_ref(ref)
    if delete_key(platform, name):
        return True
    # 旧格式兼容
    secrets = _load_secrets()
    key = f"{platform}/{name}"
    if key in secrets:
        del secrets[key]
        _save_secrets(secrets)
        return True
    return False


def list_secrets() -> list[dict]:
    """列出所有密钥元信息（兼容旧接口）。"""
    secrets = _load_secrets()
    result = []
    for platform, data in secrets.items():
        # 新格式
        for name in data.get("keys", {}).keys():
            result.append({"platform": platform, "name": name, "kind": "Key", "account": ""})
        for username in data.get("accounts", {}).keys():
            result.append({"platform": platform, "name": username, "kind": "Account", "account": username})
        # 旧格式兼容
        for flat_key, val in data.items() if isinstance(data, dict) and "accounts" not in data else []:
            if isinstance(val, dict) and "ciphertext" in val:
                p, n = flat_key.split("/", 1)
                result.append({"platform": p, "name": n, "kind": val.get("kind", ""), "account": val.get("account", "")})
    return result


def list_platforms() -> dict[str, list[dict]]:
    """按平台分组列出密钥（兼容旧接口）。"""
    secrets = _load_secrets()
    platforms = {}
    for platform, data in secrets.items():
        if platform not in platforms:
            platforms[platform] = []
        # 新格式
        for name in data.get("keys", {}).keys():
            platforms[platform].append({"name": name, "kind": "Key", "account": ""})
        for username in data.get("accounts", {}).keys():
            platforms[platform].append({"name": username, "kind": "Account", "account": username})
        # 旧格式兼容
        for flat_key, val in data.items() if isinstance(data, dict) and "accounts" not in data else []:
            if isinstance(val, dict) and "ciphertext" in val:
                platforms[platform].append({"name": flat_key.split("/", 1)[1], "kind": val.get("kind", ""), "account": val.get("account", "")})
    return platforms
