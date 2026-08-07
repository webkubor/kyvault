"""crypto 模块单元测试。"""

import os
import pytest
from kyvault.crypto import derive_key, encrypt, decrypt


class TestDeriveKey:
    def test_returns_32_bytes(self):
        import base64
        key_b64 = base64.b64encode(os.urandom(32)).decode("ascii")
        result = derive_key(key_b64)
        assert len(result) == 32

    def test_deterministic(self):
        import base64
        key_b64 = base64.b64encode(os.urandom(32)).decode("ascii")
        assert derive_key(key_b64) == derive_key(key_b64)


class TestEncryptDecrypt:
    def test_roundtrip(self):
        import base64
        master_key = base64.b64encode(os.urandom(32)).decode("ascii")
        plaintext = "ghp_xxxxxxxxxxxx"

        ciphertext = encrypt(plaintext, master_key)
        result = decrypt(ciphertext, master_key)

        assert result == plaintext

    def test_different_ciphertexts(self):
        import base64
        master_key = base64.b64encode(os.urandom(32)).decode("ascii")
        plaintext = "same_value"

        c1 = encrypt(plaintext, master_key)
        c2 = encrypt(plaintext, master_key)

        assert c1 != c2

    def test_wrong_key_fails(self):
        import base64
        master_key1 = base64.b64encode(os.urandom(32)).decode("ascii")
        master_key2 = base64.b64encode(os.urandom(32)).decode("ascii")

        ciphertext = encrypt("secret", master_key1)

        with pytest.raises(Exception):
            decrypt(ciphertext, master_key2)
