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


# ── 服务器台账与 CLI 多 Profile 选项支持 ──────────────────

def set_server(hostname: str, ip: str, root_password: str, cost: str = "", provider: str = "") -> None:
    """加密并保存服务器台账。"""
    master_key = get_master_key()
    data = {
        "ip": encrypt(ip, master_key),
        "root-password": encrypt(root_password, master_key)
    }
    if cost:
        data["cost"] = encrypt(cost, master_key)
    if provider:
        data["provider"] = encrypt(provider, master_key)
        
    secrets = _load_secrets()
    if "_servers" not in secrets:
        secrets["_servers"] = {}
    secrets["_servers"][hostname] = data
    _save_secrets(secrets)


def get_server(hostname: str, field: Optional[str] = None) -> Optional[dict | str]:
    """读取并解密服务器台账。"""
    secrets = _load_secrets()
    if "_servers" not in secrets or hostname not in secrets["_servers"]:
        return None
    
    master_key = get_master_key()
    encrypted_data = secrets["_servers"][hostname]
    
    if field:
        ciphertext = encrypted_data.get(field)
        if ciphertext is None:
            return None
        return decrypt(ciphertext, master_key)
        
    # 解密所有字段并返回 dict
    result = {}
    for k, val in encrypted_data.items():
        result[k] = decrypt(val, master_key)
    return result


def list_servers() -> list[str]:
    """列出所有服务器主机名。"""
    secrets = _load_secrets()
    if "_servers" not in secrets:
        return []
    return list(secrets["_servers"].keys())


def delete_server(hostname: str) -> bool:
    """删除服务器台账。"""
    secrets = _load_secrets()
    if "_servers" not in secrets or hostname not in secrets["_servers"]:
        return False
    del secrets["_servers"][hostname]
    _save_secrets(secrets)
    return True


def set_cli_token(cli_name: str, profile: str, token: str) -> None:
    """加密并保存 CLI 多 Profile Token。"""
    ciphertext = encrypt(token, get_master_key())
    secrets = _load_secrets()
    if "_clis" not in secrets:
        secrets["_clis"] = {}
    if cli_name not in secrets["_clis"]:
        secrets["_clis"][cli_name] = {}
    secrets["_clis"][cli_name][profile] = ciphertext
    _save_secrets(secrets)


def get_cli_token(cli_name: str, profile: str) -> Optional[str]:
    """读取并解密 CLI Token。"""
    secrets = _load_secrets()
    if "_clis" not in secrets or cli_name not in secrets["_clis"]:
        return None
    ciphertext = secrets["_clis"][cli_name].get(profile)
    if ciphertext is None:
        return None
    return decrypt(ciphertext, get_master_key())


def list_clis(cli_name: Optional[str] = None) -> list[str] | dict[str, list[str]]:
    """列出所有 CLI 或指定 CLI 下的 Profile 列表。"""
    secrets = _load_secrets()
    if "_clis" not in secrets:
        return [] if cli_name else {}
    
    if cli_name:
        if cli_name not in secrets["_clis"]:
            return []
        return list(secrets["_clis"][cli_name].keys())
        
    result = {}
    for name, profiles in secrets["_clis"].items():
        result[name] = list(profiles.keys())
    return result


def delete_cli_token(cli_name: str, profile: str) -> bool:
    """删除 CLI Token。"""
    secrets = _load_secrets()
    if "_clis" not in secrets or cli_name not in secrets["_clis"]:
        return False
    profiles = secrets["_clis"][cli_name]
    if profile in profiles:
        del profiles[profile]
        if not profiles:
            del secrets["_clis"][cli_name]
        _save_secrets(secrets)
        return True
    return False


# ── 旧接口兼容（alias / import 用） ──────────────────────

def get_secret(ref: str) -> Optional[str]:
    """读取并解密本地密钥（兼容旧 secret:// 格式）。"""
    platform, name = parse_ref(ref)
    
    # 支持 server 与 cli 命名空间通过 secret:// 寻址
    if platform == "server":
        parts = name.split("/", 1)
        if len(parts) == 2:
            return get_server(parts[0], parts[1])
        elif len(parts) == 1:
            data = get_server(parts[0])
            import json
            return json.dumps(data) if data else None
            
    if platform == "cli":
        parts = name.split("/", 1)
        if len(parts) == 2:
            return get_cli_token(parts[0], parts[1])

    secrets = _load_secrets()

    # 新格式：platform/name 在 keys 或 accounts 中
    if platform in secrets:
        # 排除系统保留前缀
        if not platform.startswith("_"):
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
    if platform == "server":
        parts = name.split("/", 1)
        if len(parts) == 2:
            # 默认写入 ip 或 root-password，其余为可选项
            ip = value if parts[1] == "ip" else ""
            pw = value if parts[1] == "root-password" else ""
            set_server(parts[0], ip, pw)
            return
    if platform == "cli":
        parts = name.split("/", 1)
        if len(parts) == 2:
            set_cli_token(parts[0], parts[1], value)
            return
            
    set_key(platform, name, value)


def delete_secret(ref: str) -> bool:
    """删除密钥（兼容旧格式）。"""
    platform, name = parse_ref(ref)
    if platform == "server":
        parts = name.split("/", 1)
        return delete_server(parts[0])
    if platform == "cli":
        parts = name.split("/", 1)
        if len(parts) == 2:
            return delete_cli_token(parts[0], parts[1])

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
    
    # 注入服务器和 CLI 字段
    if "_servers" in secrets:
        for hostname, fields in secrets["_servers"].items():
            for field in fields.keys():
                result.append({"platform": "server", "name": f"{hostname}/{field}", "kind": "Server Field", "account": ""})
                
    if "_clis" in secrets:
        for cli_name, profiles in secrets["_clis"].items():
            for profile in profiles.keys():
                result.append({"platform": "cli", "name": f"{cli_name}/{profile}", "kind": "CLI Token", "account": profile})

    for platform, data in secrets.items():
        if platform.startswith("_"):
            continue
        # 新格式
        if isinstance(data, dict) and ("keys" in data or "accounts" in data):
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
    
    # 注入系统内部虚平台以实现对齐显示
    if "_servers" in secrets:
        platforms["server"] = [{"name": f"{h}/{f}", "kind": "Server Field", "account": ""} for h, fields in secrets["_servers"].items() for f in fields.keys()]
    if "_clis" in secrets:
        platforms["cli"] = [{"name": f"{c}/{p}", "kind": "CLI Token", "account": p} for c, profiles in secrets["_clis"].items() for p in profiles.keys()]

    for platform, data in secrets.items():
        if platform.startswith("_"):
            continue
        if platform not in platforms:
            platforms[platform] = []
        # 新格式
        if isinstance(data, dict) and ("keys" in data or "accounts" in data):
            for name in data.get("keys", {}).keys():
                platforms[platform].append({"name": name, "kind": "Key", "account": ""})
            for username in data.get("accounts", {}).keys():
                platforms[platform].append({"name": username, "kind": "Account", "account": username})
        # 旧格式兼容
        for flat_key, val in data.items() if isinstance(data, dict) and "accounts" not in data else []:
            if isinstance(val, dict) and "ciphertext" in val:
                platforms[platform].append({"name": flat_key.split("/", 1)[1], "kind": val.get("kind", ""), "account": val.get("account", "")})
    return platforms
