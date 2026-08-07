"""alias 模块单元测试。"""

from pathlib import Path
from unittest.mock import patch

import pytest
from kyvault.alias import (
    set_alias,
    get_alias,
    resolve_ref,
    list_aliases,
    delete_alias,
)


class TestAlias:
    @pytest.fixture
    def tmp_aliases(self, tmp_path):
        aliases_file = tmp_path / "aliases.json"
        with patch("keyring.alias.ALIASES_FILE", aliases_file):
            yield aliases_file

    def test_set_and_get(self, tmp_aliases):
        set_alias("github_token", "secret://github/my-pat")
        result = get_alias("github_token")
        assert result == "secret://github/my-pat"

    def test_get_nonexistent(self):
        result = get_alias("nonexistent")
        assert result is None

    def test_resolve_ref_alias(self, tmp_aliases):
        set_alias("github_token", "secret://github/my-pat")
        result = resolve_ref("github_token")
        assert result == "secret://github/my-pat"

    def test_resolve_ref_secret(self):
        result = resolve_ref("secret://github/my-pat")
        assert result == "secret://github/my-pat"

    def test_resolve_ref_unknown(self):
        result = resolve_ref("unknown")
        assert result == "unknown"

    def test_list_aliases(self, tmp_aliases):
        set_alias("github_token", "secret://github/pat")
        set_alias("gmail_pass", "secret://gmail/pass")

        aliases = list_aliases()
        assert len(aliases) == 2
        assert aliases["github_token"] == "secret://github/pat"

    def test_delete_alias(self, tmp_aliases):
        set_alias("github_token", "secret://github/pat")
        assert delete_alias("github_token") is True
        assert get_alias("github_token") is None

    def test_delete_nonexistent(self):
        assert delete_alias("nonexistent") is False
