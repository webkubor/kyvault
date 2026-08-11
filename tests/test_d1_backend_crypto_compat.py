"""跨语言加密兼容性测试。

验证 kyvault.d1_backend 的 AES-256-GCM 实现跟 CortexOS Go 端
(pkg/infra/secretvault/vault.go) 字节级兼容——同一把 key、同一个 nonce、
同样的明文，双方必须能读出彼此的密文，否则 D1 里已有的 108+ 条密钥会在
切到 Python D1 backend 后读不出来。

下面这组向量是直接用一份等价的 Go 程序（crypto/aes + crypto/cipher.NewGCM，
跟 vault.go 里 newGCM/encrypt 完全同样的调用方式）离线算出来的固定值，不是
Python 自己造的自洽向量。
"""

import base64

from kyvault.d1_backend import _decrypt, _encrypt
import kyvault.d1_backend as d1_backend

# Go 端固定输入：key = bytes(0..31)，nonce = "123456789012"（12 字节 ASCII）
_KEY_B64 = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8="
_NONCE_B64 = "MTIzNDU2Nzg5MDEy"
_PLAINTEXT = "test-secret-value-1234567890"
# 上面这组 key/nonce/plaintext 喂给 Go 的 aes.NewCipher + cipher.NewGCM +
# gcm.Seal(nil, nonce, plaintext, nil) 算出的密文（AAD=nil）。
_GO_CIPHERTEXT_B64 = "IFRVe8M3nfgbQ3xat0bsEzodak92uoF/ur35DFwh0ElnJ7PaQMQIwWhNl04="


def _with_fixed_master_key(monkeypatch):
    monkeypatch.setattr(d1_backend, "_MASTER_KEY_CACHE", base64.b64decode(_KEY_B64))


class TestCrossLanguageCryptoCompat:
    def test_python_decrypts_go_produced_ciphertext(self, monkeypatch):
        """Go 加密的密文，Python D1 backend 必须能正确解密。"""
        _with_fixed_master_key(monkeypatch)
        plaintext = _decrypt(_GO_CIPHERTEXT_B64, _NONCE_B64)
        assert plaintext == _PLAINTEXT

    def test_go_can_decrypt_python_produced_ciphertext(self, monkeypatch):
        """Python D1 backend 加密的密文，格式必须是 Go 能解的形状：
        nonce 和 ciphertext 分开的两个 base64 字段，且用同一把原始 32 字节 key
        （不经过 SHA-256 派生）直接做 AES-256-GCM，没有 AAD。
        这里不跑真的 Go 进程，而是验证 Python 自己算出来的密文用同一把 key/nonce
        重新解密能正确还原——并且额外验证：用固定 nonce 时 Python 产出的密文长度
        和 Go 产出的密文长度完全一致（GCM 密文 = 明文长度 + 16 字节 tag，
        两边算法/tag 长度必须一致，这是跨语言互通的前提）。
        """
        _with_fixed_master_key(monkeypatch)
        ciphertext_b64, nonce_b64 = _encrypt(_PLAINTEXT)
        # 长度必须跟 Go 产出的密文长度一致（同样是 GCM，16 字节 tag）
        assert len(base64.b64decode(ciphertext_b64)) == len(base64.b64decode(_GO_CIPHERTEXT_B64))
        # 用自己加密的密文自己解密，确认往返正确
        assert _decrypt(ciphertext_b64, nonce_b64) == _PLAINTEXT

    def test_master_key_used_directly_without_sha256_derivation(self, monkeypatch):
        """D1 backend 的 key 必须是 D1 里存的原始 32 字节，不能像本地文件后端
        (crypto.py 的 derive_key) 那样再套一层 SHA-256——否则跟 Go 侧对不上。"""
        _with_fixed_master_key(monkeypatch)
        key = d1_backend._master_key()
        assert key == base64.b64decode(_KEY_B64)
        assert len(key) == 32
