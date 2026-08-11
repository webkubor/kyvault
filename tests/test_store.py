"""store 模块单元测试。"""

import base64
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
    def tmp_store(self, tmp_path, monkeypatch):
        secrets_file = tmp_path / "secrets.json"
        test_key = base64.b64encode(os.urandom(32)).decode("ascii")
        monkeypatch.setenv("KEYRING_MASTER_KEY", test_key)
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


class TestServerAndCliStore:
    @pytest.fixture
    def tmp_store(self, tmp_path, monkeypatch):
        secrets_file = tmp_path / "secrets.json"
        test_key = base64.b64encode(os.urandom(32)).decode("ascii")
        monkeypatch.setenv("KEYRING_MASTER_KEY", test_key)
        with patch("kyvault.store.SECRETS_FILE", secrets_file):
            yield secrets_file

    def test_server_set_and_get(self, tmp_store):
        from kyvault.store import set_server, get_server, list_servers, delete_server
        set_server("my-host", "1.2.3.4", "root_pass", "99/mo", "tencent")
        
        # Get whole dict
        data = get_server("my-host")
        assert data["ip"] == "1.2.3.4"
        assert data["root-password"] == "root_pass"
        assert data["cost"] == "99/mo"
        assert data["provider"] == "tencent"

        # Get single field
        assert get_server("my-host", "ip") == "1.2.3.4"
        assert get_server("my-host", "cost") == "99/mo"

        # List and delete
        assert "my-host" in list_servers()
        assert delete_server("my-host") is True
        assert get_server("my-host") is None

    def test_cli_set_and_get(self, tmp_store):
        from kyvault.store import set_cli_token, get_cli_token, list_clis, delete_cli_token
        set_cli_token("studio-cli", "webkubor", "token_123")
        assert get_cli_token("studio-cli", "webkubor") == "token_123"
        assert "webkubor" in list_clis("studio-cli")
        assert delete_cli_token("studio-cli", "webkubor") is True
        assert get_cli_token("studio-cli", "webkubor") is None

    def test_generic_secret_compatibility(self, tmp_store):
        from kyvault.store import set_server, set_cli_token
        set_server("my-host", "1.2.3.4", "root_pass")
        set_cli_token("studio-cli", "webkubor", "token_123")

        # Query via secret:// format
        assert get_secret("secret://server/my-host/ip") == "1.2.3.4"
        assert get_secret("secret://server/my-host/root-password") == "root_pass"
        assert get_secret("secret://cli/studio-cli/webkubor") == "token_123"

        # List secrets check
        secrets = list_secrets()
        paths = [s["platform"] + "/" + s["name"] for s in secrets]
        assert "server/my-host/ip" in paths
        assert "cli/studio-cli/webkubor" in paths
