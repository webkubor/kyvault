#!/usr/bin/env python3
"""把 Chrome 导出的密码 CSV 分类导入 kyvault。

## 用法

1. Chrome → 设置 → 自动填充 → 密码管理器 → ⋯ → 导出密码
   （会弹一次 Mac 登录验证 —— 你自己授权。产出一个 CSV）
2. python3 import-chrome-csv.py ~/Downloads/Chrome\ Passwords.csv [--dry-run]
3. 导入完立刻删掉 CSV：它是明文，别留在下载目录

## 设计约束

- 密码永远走 stdin 传给 kyvault set，不进命令行参数、不进 shell history
- 本脚本自己也**绝不打印密码**，只打印域名/用户名/成功与否
- 命名：secret://web/<域名>/<用户名>，账号备注存完整 URL
- 已存在的同名条目**跳过**（--overwrite 才覆盖）—— 避免把手动改过的
  密码被一次批量导入盖回旧值
"""
import argparse
import csv
import re
import subprocess
import sys
from urllib.parse import urlparse


def platform_of(url: str) -> str:
    host = urlparse(url).hostname or "unknown"
    # 去掉 www. 和端口，取主域做平台名（accounts.google.com → google.com）
    host = host.split(":")[0].removeprefix("www.")
    parts = host.split(".")
    return ".".join(parts[-2:]) if len(parts) >= 2 else host


def slug(s: str) -> str:
    return re.sub(r"[^A-Za-z0-9_.@-]+", "-", s).strip("-") or "unknown"


def existing_refs() -> set:
    r = subprocess.run(["kyvault", "list"], capture_output=True, text=True)
    return {ln.split()[0] for ln in r.stdout.splitlines() if ln.startswith("secret://")}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("csv_path")
    ap.add_argument("--dry-run", action="store_true", help="只看会导入什么，不写入")
    ap.add_argument("--overwrite", action="store_true", help="同名条目也覆盖")
    args = ap.parse_args()

    have = set() if args.dry_run else existing_refs()
    added = skipped = empty = 0

    with open(args.csv_path, newline="", encoding="utf-8") as f:
        for row in csv.DictReader(f):
            pw = (row.get("password") or "").strip()
            url = (row.get("url") or "").strip()
            user = (row.get("username") or "").strip()
            if not pw:
                empty += 1
                continue
            plat = f"web/{platform_of(url)}"
            name = slug(user or platform_of(url))
            ref = f"secret://{plat}/{name}"
            if ref in have and not args.overwrite:
                skipped += 1
                continue
            # 不打印密码，只打印身份
            print(f"{'[预演] ' if args.dry_run else ''}{ref}  ({user or '—'})")
            if not args.dry_run:
                p = subprocess.run(
                    ["kyvault", "set", "--kind", "密码",
                     "--platform", plat, "--name", name,
                     "--account", url, "--value", "-"],
                    input=pw, text=True, capture_output=True,
                )
                if p.returncode != 0:
                    print(f"  ✗ 失败: {p.stderr.strip()[:80]}", file=sys.stderr)
                    continue
            added += 1

    print(f"\n{'（预演）' if args.dry_run else ''}导入 {added} 条 · 跳过已存在 {skipped} · 空密码 {empty}")
    if not args.dry_run:
        print("⚠️ 现在删掉那个 CSV：它是明文密码。 rm 后清空废纸篓。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
