"""后台版本检查 — 每天最多查一次 GitHub Releases，静默失败，从不阻断命令执行。"""

import json
import time
import urllib.request
from pathlib import Path

from .store import SECRETS_DIR

CHECK_FILE = SECRETS_DIR / ".last_update_check"
CHECK_INTERVAL = 24 * 60 * 60  # 一天查一次
LATEST_RELEASE_URL = "https://api.github.com/repos/webkubor/kyvault/releases/latest"


def _parse_version(v: str) -> tuple:
    return tuple(int(p) for p in v.lstrip("v").split("."))


def _should_check() -> bool:
    if not CHECK_FILE.exists():
        return True
    try:
        last = float(CHECK_FILE.read_text().strip())
    except (ValueError, OSError):
        return True
    return time.time() - last > CHECK_INTERVAL


def _mark_checked() -> None:
    try:
        SECRETS_DIR.mkdir(parents=True, exist_ok=True)
        CHECK_FILE.write_text(str(time.time()))
    except OSError:
        pass


def check_for_update(current_version: str) -> None:
    """静默检查新版本，有更新才打印一行提示到 stderr；任何失败都直接忽略。"""
    if not _should_check():
        return
    try:
        req = urllib.request.Request(
            LATEST_RELEASE_URL, headers={"Accept": "application/vnd.github+json"}
        )
        with urllib.request.urlopen(req, timeout=2) as resp:
            data = json.load(resp)
        latest = data.get("tag_name", "").strip()
        _mark_checked()
        if latest and _parse_version(latest) > _parse_version(current_version):
            import sys
            print(
                f"🔔 kyvault 有新版本 {latest}（当前 v{current_version}）："
                f"https://github.com/webkubor/kyvault/releases",
                file=sys.stderr,
            )
    except Exception:
        # 网络失败、限流、格式变化……一律静默，不影响正常命令
        _mark_checked()
