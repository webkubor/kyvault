"""AES-256-GCM 加解密模块。"""

import base64
import hashlib
import os

from cryptography.hazmat.primitives.ciphers.aead import AESGCM


def derive_key(master_key_b64: str) -> bytes:
    """从 base64 编码的 master key 派生 32 字节 AES-256 密钥。"""
    raw = base64.b64decode(master_key_b64)
    return hashlib.sha256(raw).digest()


def encrypt(plaintext: str, master_key_b64: str, associated_data: bytes = b"") -> str:
    """AES-256-GCM 加密，返回 base64 编码的密文（含 nonce）。"""
    key = derive_key(master_key_b64)
    aesgcm = AESGCM(key)
    nonce = os.urandom(12)
    ciphertext = aesgcm.encrypt(nonce, plaintext.encode("utf-8"), associated_data)
    return base64.b64encode(nonce + ciphertext).decode("ascii")


def decrypt(ciphertext_b64: str, master_key_b64: str, associated_data: bytes = b"") -> str:
    """AES-256-GCM 解密，输入 base64 编码的密文（含 nonce）。"""
    key = derive_key(master_key_b64)
    data = base64.b64decode(ciphertext_b64)
    nonce = data[:12]
    ciphertext = data[12:]
    aesgcm = AESGCM(key)
    plaintext = aesgcm.decrypt(nonce, ciphertext, associated_data)
    return plaintext.decode("utf-8")
