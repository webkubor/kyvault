#!/usr/bin/env bash
#
# 一键把 Chrome 导出的密码导入 kyvault。跑一条命令即可：
#
#     ~/dev/github/app/kyvault/tools/import-passwords.sh
#
# 它会：自动找到 Downloads 里最新的密码 CSV → 先预演给你看 →
# 你确认后真导入 → 问你要不要删掉 CSV。
#
# 前提：你已经在 Chrome 里导出过密码
#   Chrome → 设置 → 自动填充 → 密码管理器 → 右上 ⋯ → 导出密码
#   （会弹一次 Mac 登录验证，你自己授权；产出一个 CSV 到 Downloads）
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"

# 找 Downloads 里最新的、名字像密码导出的 CSV
CSV=$(ls -t ~/Downloads/*[Pp]assword*.csv ~/Downloads/*[Mm]ima*.csv 2>/dev/null | head -1 || true)
if [ -z "${CSV:-}" ]; then
  echo "✗ 在 ~/Downloads 里没找到密码 CSV。"
  echo "  先去 Chrome：设置 → 自动填充 → 密码管理器 → ⋯ → 导出密码"
  exit 1
fi
echo "找到：$CSV"
echo

echo "=== 预演（不写入，看看会导入哪些）==="
python3 "$HERE/import-chrome-csv.py" "$CSV" --dry-run
echo

printf "确认导入以上条目到 kyvault？[y/N] "
read -r ans
if [ "$ans" != "y" ] && [ "$ans" != "Y" ]; then
  echo "已取消，什么都没写。"
  exit 0
fi

echo
echo "=== 正式导入 ==="
python3 "$HERE/import-chrome-csv.py" "$CSV"
echo

printf "导入完成。现在删掉这个明文 CSV？[Y/n] "
read -r del
if [ "$del" != "n" ] && [ "$del" != "N" ]; then
  rm -f "$CSV"
  echo "✓ 已删除 $CSV"
else
  echo "⚠️ CSV 留着了，它是明文密码，记得自己删。"
fi
