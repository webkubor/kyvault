#!/usr/bin/env bash
# 一键生成「Kyvault Dock App」：点 Dock 图标 = 打开 ky ui（本地 Web GUI）。
#
# 原理：用 osacompile 把一段 AppleScript 编译成真正的 .app，
# 双击/点 Dock 时启动 ky ui 并自动唤起浏览器。
# 不签名、不公证 —— 它只是你自己机器上的快捷方式，不出本机。
#
# 用法：bash scripts/make-dock-app.sh          # 生成到 ~/Applications/Kyvault GUI.app
#       bash scripts/make-dock-app.sh --open   # 生成后立刻打开一次
# 可用 KY_BIN=... 指定 ky 的绝对路径（默认自动探测）。

set -euo pipefail

DEST_DIR="${HOME}/Applications"
APP_NAME="Kyvault GUI.app"
KY_BIN="${KY_BIN:-}"

# 找 ky：KY_BIN → PATH → 常见安装位置
if [[ -z "$KY_BIN" ]]; then
  if command -v ky >/dev/null 2>&1; then
    KY_BIN="$(command -v ky)"
  elif [[ -x "$HOME/.local/bin/ky" ]]; then
    KY_BIN="$HOME/.local/bin/ky"
  elif [[ -x "/opt/homebrew/bin/ky" ]]; then
    KY_BIN="/opt/homebrew/bin/ky"
  elif [[ -x "/usr/local/bin/ky" ]]; then
    KY_BIN="/usr/local/bin/ky"
  else
    echo "错误：找不到 ky，请先安装 kyvault（curl 安装脚本或 cargo install kyvault）" >&2
    exit 1
  fi
fi
KY_BIN="$(eval echo "$KY_BIN")"   # 展开 ~
echo "ky 位置：$KY_BIN"

# 路径在编译期写进脚本本体（applet 双击时没有 shell 环境，system attribute 不可靠）
# 已有实例在跑时不再起第二个 —— ky ui 会自动换端口，起两个反而困扰
TMP_APP="$(mktemp -d)/${APP_NAME}"
osacompile -o "$TMP_APP" -e "
if (do shell script \"pgrep -f '${KY_BIN} ui' >/dev/null 2>&1 && echo yes || echo no\") is \"yes\" then
  display notification \"ky ui 已在运行（看浏览器标签页）\" with title \"Kyvault GUI\"
else
  do shell script \"${KY_BIN} ui >/dev/null 2>&1 &\"
end if" >/dev/null

rm -rf "${DEST_DIR:?}/${APP_NAME}"
mkdir -p "$DEST_DIR"
mv "$TMP_APP" "$DEST_DIR/"
echo "已生成：${DEST_DIR}/${APP_NAME}"
echo "拖进 Dock 即可一键打开 ky ui。"

if [[ "${1:-}" == "--open" ]]; then
  open "${DEST_DIR}/${APP_NAME}"
fi
