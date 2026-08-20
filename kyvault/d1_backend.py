"""Cloudflare D1 远程后端 —— 与 CortexOS `cs kyvault`（Go 实现，pkg/infra/secretvault）
共用同一张 secret_vault 表、同一套 AES-256-GCM wire format，做到密文互相可读、零数据迁移。

与本地文件后端（store.py 默认行为）的关键差异：
- nonce 和 ciphertext 分两列各自 base64 存储（不是拼接成一个 base64 字符串）
- master key 是 D1 site_config 表里的原始 32 字节 AES key（不经过 SHA-256 再派生）
- master key 只读：这里不会自动生成，必须已经由 Go 侧（或先跑一次 `cs kyvault`）初始化好

启用方式：设置环境变量 KYVAULT_BACKEND=d1，并提供：
  KYVAULT_D1_ACCOUNT_ID   Cloudflare 账号 ID
  KYVAULT_D1_DATABASE_ID  D1 数据库 ID
  CLOUDFLARE_API_TOKEN 或 CF_API_TOKEN  有权限读写该 D1 库的 Cloudflare API Token
"""

import base64
import hashlib
import json
import os
import time
import urllib.error
import urllib.request
from typing import Optional

from cryptography.hazmat.primitives.ciphers.aead import AESGCM

D1_QUERY_TIMEOUT = 15
_MASTER_KEY_CACHE: Optional[bytes] = None


class D1ConfigError(RuntimeError):
    """KYVAULT_BACKEND=d1 但缺少必要配置时抛出。"""


class D1QueryError(RuntimeError):
    """D1 查询返回错误时抛出。"""


def _config() -> tuple[str, str, str]:
    account_id = os.environ.get("KYVAULT_D1_ACCOUNT_ID", "")
    database_id = os.environ.get("KYVAULT_D1_DATABASE_ID", "")
    token = os.environ.get("CLOUDFLARE_API_TOKEN") or os.environ.get("CF_API_TOKEN") or ""
    missing = [
        name
        for name, value in (
            ("KYVAULT_D1_ACCOUNT_ID", account_id),
            ("KYVAULT_D1_DATABASE_ID", database_id),
            ("CLOUDFLARE_API_TOKEN 或 CF_API_TOKEN", token),
        )
        if not value
    ]
    if missing:
        raise D1ConfigError(f"KYVAULT_BACKEND=d1 缺少环境变量：{', '.join(missing)}")
    return account_id, database_id, token


def _query(sql: str, params: Optional[list] = None) -> dict:
    """POST 到 Cloudflare D1 HTTP API，跟 Go 端 D1Client.Query 完全同一份契约。"""
    account_id, database_id, token = _config()
    url = f"https://api.cloudflare.com/client/v4/accounts/{account_id}/d1/database/{database_id}/query"
    payload = json.dumps({"sql": sql, "params": params or []}).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=payload,
        method="POST",
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=D1_QUERY_TIMEOUT) as resp:
            body = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        body = json.loads(e.read().decode("utf-8"))
    except urllib.error.URLError as e:
        raise D1QueryError(f"D1 请求失败（网络层）：{e}") from e

    if not body.get("success"):
        errors = body.get("errors") or [{"message": "未知错误", "code": 0}]
        raise D1QueryError(f"D1 错误: {errors[0].get('message')} (code {errors[0].get('code')})")
    return body


def _rows(body: dict) -> list[dict]:
    rows: list[dict] = []
    for group in body.get("result", []):
        rows.extend(group.get("results", []))
    return rows


def _master_key() -> bytes:
    """从 D1 site_config 读取 32 字节原始 AES key（不做二次派生）。只读，不在这里生成。"""
    global _MASTER_KEY_CACHE
    if _MASTER_KEY_CACHE is not None:
        return _MASTER_KEY_CACHE
    body = _query(
        "SELECT value FROM site_config WHERE key = ?1 LIMIT 1",
        ["secret_vault_master_key"],
    )
    rows = _rows(body)
    if not rows:
        raise D1ConfigError(
            "D1 的 site_config 里还没有 secret_vault_master_key。"
            "先用 Go 端 `cs kyvault set` 写入至少一条密钥完成初始化，再用这个 D1 backend。"
        )
    encoded = str(rows[0].get("value", "")).strip()
    key = base64.b64decode(encoded)
    if len(key) != 32:
        raise D1ConfigError("D1 里的 secret_vault_master_key 长度不是 32 字节，可能已损坏")
    _MASTER_KEY_CACHE = key
    return key


def _encrypt(plaintext: str) -> tuple[str, str]:
    """返回 (ciphertext_b64, nonce_b64)，跟 Go vault.go 的 encrypt() 字节级兼容。"""
    key = _master_key()
    nonce = os.urandom(12)
    ciphertext = AESGCM(key).encrypt(nonce, plaintext.encode("utf-8"), None)
    return (
        base64.b64encode(ciphertext).decode("ascii"),
        base64.b64encode(nonce).decode("ascii"),
    )


def _decrypt(ciphertext_b64: str, nonce_b64: str) -> str:
    key = _master_key()
    ciphertext = base64.b64decode(ciphertext_b64)
    nonce = base64.b64decode(nonce_b64)
    plaintext = AESGCM(key).decrypt(nonce, ciphertext, None)
    return plaintext.decode("utf-8")


def _last4(value: str) -> str:
    return value if len(value) <= 4 else value[-4:]


# ── 对外接口：跟 store.py 里 get_secret/set_secret/... 一一对应 ──────────

def get_secret(ref: str) -> Optional[str]:
    body = _query("SELECT ciphertext, nonce FROM secret_vault WHERE id = ?1 LIMIT 1", [ref])
    rows = _rows(body)
    if not rows:
        return None
    return _decrypt(str(rows[0]["ciphertext"]), str(rows[0]["nonce"]))


def set_secret(ref: str, value: str, kind: str = "API Key", account: str = "") -> None:
    if not ref.startswith("secret://"):
        raise ValueError(f"格式错误，应为 secret://platform/name: {ref}")
    platform, name = ref[len("secret://"):].split("/", 1)
    ciphertext_b64, nonce_b64 = _encrypt(value)
    now = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
    sha256_hex = hashlib.sha256(value.encode("utf-8")).hexdigest()
    _query(
        """INSERT INTO secret_vault
            (id, kind, platform, name, account, ciphertext, nonce, length, sha256, last4, source, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(id) DO UPDATE SET
                kind=excluded.kind, platform=excluded.platform, name=excluded.name,
                account=excluded.account, ciphertext=excluded.ciphertext, nonce=excluded.nonce,
                length=excluded.length, sha256=excluded.sha256, last4=excluded.last4,
                source=excluded.source, updated_at=excluded.updated_at""",
        [
            ref, kind, platform, name, account,
            ciphertext_b64, nonce_b64, len(value), sha256_hex, _last4(value),
            "user_input", now, now,
        ],
    )


def annotate_secret(ref: str, account: str | None = None, kind: str | None = None) -> bool:
    """只改元信息（备注 account / 类型 kind），**不碰密文**。

    为什么单开一个函数而不复用 set_secret：set_secret 是全量 upsert，改一句备注要求调用方
    先把密钥明文找出来重新交一遍。那既让明文多走一趟（每一趟都是一次泄漏机会），
    又在「手上没有明文」时根本做不到——而备注恰恰是最常需要订正的字段：
    2026-08-20 一条 volcengine key 的备注写成了「小楠主模型 glm-5.2」，实际它是图片逆向
    上游用的，看备注的人会以为动它会影响小楠，不敢清理。这种订正不该要求交出密钥。

    只 UPDATE 传了的列，ciphertext/nonce/sha256/last4/length 一概不动。
    返回 False 表示该 ref 不存在（不静默当成功，否则打错一个字就以为改好了）。
    """
    if not ref.startswith("secret://"):
        raise ValueError(f"格式错误，应为 secret://platform/name: {ref}")
    if account is None and kind is None:
        raise ValueError("至少要给 --account 或 --kind 之一，否则这次调用什么都不会改")
    if not _rows(_query("SELECT id FROM secret_vault WHERE id = ?1 LIMIT 1", [ref])):
        return False
    sets, params = [], []
    if account is not None:
        sets.append(f"account=?{len(params) + 1}")
        params.append(account)
    if kind is not None:
        sets.append(f"kind=?{len(params) + 1}")
        params.append(kind)
    sets.append(f"updated_at=?{len(params) + 1}")
    params.append(time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()))
    params.append(ref)
    _query(f"UPDATE secret_vault SET {', '.join(sets)} WHERE id = ?{len(params)}", params)
    return True


def delete_secret(ref: str) -> bool:
    existing = _query("SELECT id FROM secret_vault WHERE id = ?1 LIMIT 1", [ref])
    if not _rows(existing):
        return False
    _query("DELETE FROM secret_vault WHERE id = ?1", [ref])
    return True


def list_secrets() -> list[dict]:
    body = _query(
        "SELECT platform, name, kind, account, last4, length, updated_at "
        "FROM secret_vault ORDER BY platform, name"
    )
    return [
        {
            "platform": row["platform"],
            "name": row["name"],
            "kind": row.get("kind", ""),
            "account": row.get("account") or "",
            "last4": row.get("last4", ""),
            "length": row.get("length", 0),
            "updated_at": row.get("updated_at", ""),
        }
        for row in _rows(body)
    ]


def list_platforms() -> dict[str, list[dict]]:
    platforms: dict[str, list[dict]] = {}
    for item in list_secrets():
        platforms.setdefault(item["platform"], []).append(
            {"name": item["name"], "kind": item["kind"], "account": item["account"]}
        )
    return platforms
