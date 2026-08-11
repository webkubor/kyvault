import sys
import os
import subprocess
import urllib.request
import json
from . import __version__

def get_latest_version() -> str:
    """获取 PyPI 上的最新版本，失败则退回 GitHub Releases API"""
    pypi_url = "https://pypi.org/pypi/kyvault/json"
    github_url = "https://api.github.com/repos/webkubor/kyvault/releases/latest"
    
    # 尝试 PyPI
    try:
        req = urllib.request.Request(pypi_url)
        with urllib.request.urlopen(req, timeout=3) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            return data.get("info", {}).get("version", "").strip()
    except Exception:
        pass

    # 尝试 GitHub
    try:
        req = urllib.request.Request(github_url, headers={"Accept": "application/vnd.github+json"})
        with urllib.request.urlopen(req, timeout=3) as resp:
            data = json.loads(resp.read().decode("utf-8"))
            return data.get("tag_name", "").strip().lstrip("v")
    except Exception:
        pass
        
    return ""

def _parse_version(v: str) -> tuple:
    return tuple(int(p) for p in v.lstrip("v").split("."))

def run_update() -> None:
    print("🔄 正在检查 Kyvault 在线最新版本...")
    latest = get_latest_version()
    if not latest:
        print("❌ 检查更新失败：网络连接超时或源不可达。")
        sys.exit(1)
        
    current = __version__
    if _parse_version(latest) <= _parse_version(current):
        print(f"✅ 您当前已是最新版本：v{current}")
        return

    print(f"🔥 发现新版本：v{current} -> v{latest}")
    ans = input(f"确定要升级到最新的 v{latest} 吗？[y/N]: ").strip().lower()
    if ans not in ("y", "yes"):
        print("已取消升级。")
        return

    print("🚀 正在运行安装升级包...")
    # 判断是否在 uv 虚拟环境
    cmd = [sys.executable, "-m", "pip", "install", "--upgrade", "kyvault"]
    
    # 检查是否有 uv 可用
    if subprocess.run(["which", "uv"], capture_output=True).returncode == 0:
        print("⚡ 检测到系统中安装了 uv，将使用 uv 极速升级...")
        if os.environ.get("VIRTUAL_ENV"):
            cmd = ["uv", "pip", "install", "--upgrade", "kyvault"]
        else:
            cmd = ["uv", "tool", "upgrade", "kyvault"]
            
    try:
        result = subprocess.run(cmd)
        if result.returncode == 0:
            print(f"🎉 成功升级到版本 v{latest}！")
        else:
            print("❌ 升级失败，请手动运行：pip install --upgrade kyvault")
    except Exception as e:
        print(f"❌ 升级执行出错：{e}")
        print("请手动运行：pip install --upgrade kyvault")
