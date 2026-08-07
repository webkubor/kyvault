"""import_env 模块单元测试。"""

import os
import tempfile
from pathlib import Path
from unittest.mock import patch

import pytest
from kyvault.import_env import (
    _parse_env_file,
    _guess_platform,
    _guess_kind,
    _make_name,
    import_env_file,
)


class TestParseEnvFile:
    def test_basic_parsing(self, tmp_path):
        env_file = tmp_path / ".env"
        env_file.write_text('GITHUB_TOKEN=ghp_test\nGITHUB_SECRET="secret123"\n')

        result = _parse_env_file(str(env_file))
        assert result["GITHUB_TOKEN"] == "ghp_test"
        assert result["GITHUB_SECRET"] == "secret123"

    def test_skip_comments(self, tmp_path):
        env_file = tmp_path / ".env"
        env_file.write_text("# Comment\nGITHUB_TOKEN=ghp_test\n")

        result = _parse_env_file(str(env_file))
        assert "GITHUB_TOKEN" in result
        assert "# Comment" not in result

    def test_nonexistent_file(self):
        result = _parse_env_file("/nonexistent/.env")
        assert result == {}


class TestGuessPlatform:
    def test_github(self):
        assert _guess_platform("GITHUB_TOKEN") == "github"

    def test_gitlab(self):
        assert _guess_platform("GITLAB_TOKEN") == "gitlab"

    def test_deepseek(self):
        assert _guess_platform("DEEPSEEK_API_KEY") == "deepseek"

    def test_unknown(self):
        assert _guess_platform("MY_CUSTOM_VAR") == "custom"


class TestGuessKind:
    def test_token(self):
        assert _guess_kind("GITHUB_TOKEN") == "Token"

    def test_key(self):
        assert _guess_kind("API_KEY") == "API Key"

    def test_password(self):
        assert _guess_kind("DB_PASSWORD") == "Password"


class TestMakeName:
    def test_with_platform_prefix(self):
        assert _make_name("GITHUB_TOKEN", "github") == "token"

    def test_without_prefix(self):
        assert _make_name("MY_TOKEN", "custom") == "my-token"


class TestImportEnvFile:
    def test_dry_run(self, tmp_path):
        env_file = tmp_path / ".env"
        env_file.write_text("GITHUB_TOKEN=ghp_test\n")

        result = import_env_file(str(env_file), dry_run=True)
        assert len(result) == 1
        assert result[0]["env_key"] == "GITHUB_TOKEN"

    def test_with_prefix_filter(self, tmp_path):
        env_file = tmp_path / ".env"
        env_file.write_text("GITHUB_TOKEN=ghp_test\nGITLAB_TOKEN=glpat_test\n")

        result = import_env_file(str(env_file), prefix="GITHUB_", dry_run=True)
        assert len(result) == 1
        assert result[0]["env_key"] == "GITHUB_TOKEN"
