"""store 模块单元测试。"""

import json
import os
import tempfile
from pathlib import Path
from unittest.mock import patch

import pytest
from kyvault.store import (
    parse_ref,
    _load_secrets,
    _save_secrets,
    set_secret,
    get_secret,
    delete_secret,
    list_secrets,
    list_platforms,
)


class TestParseRef:
    def test_valid_ref(self):
        platform, name = parse_ref("secret://github/my-pat")
        assert platform == "github"
        assert name == "my-pat"

    def test_invalid_prefix(self):
        with pytest.raises(ValueError, match="格式错误"):
            parse_ref("github/my-pat")

    def test_invalid_format(self):
        with pytest.raises(ValueError, match="格式错误"):
            parse_ref("secret://github")


class TestSecretsStore:
    @pytest.fixture
    def tmp_store(self, tmp_path):
        secrets_file = tmp_path / "secrets.json"
        with patch("kyvault.store.SECRETS_FILE", secrets_file):
            yield secrets_file

    def test_set_and_get(self, tmp_store):
        set_secret("secret://github/my-pat", "ghp_test123")
        result = get_secret("secret://github/my-pat")
        assert result == "ghp_test123"

    def test_get_nonexistent(self):
        result = get_secret("secret://nonexistent/key")
        assert result is None

    def test_delete(self, tmp_store):
        set_secret("secret://github/my-pat", "ghp_test123")
        assert delete_secret("secret://github/my-pat") is True
        assert get_secret("secret://github/my-pat") is None

    def test_delete_nonexistent(self):
        assert delete_secret("secret://nonexistent/key") is False

    def test_list_secrets(self, tmp_store):
        set_secret("secret://github/pat1", "val1", kind="Token")
        set_secret("secret://github/pat2", "val2", kind="Token")
        set_secret("secret://gmail/pass", "val3", kind="Password")

        secrets = list_secrets()
        assert len(secrets) == 3

    def test_list_platforms(self, tmp_store):
        set_secret("secret://github/pat1", "val1")
        set_secret("secret://github/pat2", "val2")
        set_secret("secret://gmail/pass", "val3")

        platforms = list_platforms()
        assert "github" in platforms
        assert "gmail" in platforms
        assert len(platforms["github"]) == 2
        assert len(platforms["gmail"]) == 1
