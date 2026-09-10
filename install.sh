#!/usr/bin/env bash
# kyvault 安装器（Rust 版）。
#
# 新装：下载对应平台的静态二进制，不需要 Python、也不需要 Rust 工具链。
# 升级：识别并清掉 Python 版留下的入口，**但绝不碰 ~/.keyring 里的
#       master.key 与 secrets.json** —— 两版的加密格式完全兼容
#       （tests/interop.rs 双向验证过），换的只是二进制，数据原地不动。
set -euo pipefail

REPO="webkubor/kyvault"
BIN_NAME="kyvault"
INSTALL_DIR="${KYVAULT_INSTALL_DIR:-$HOME/.local/bin}"
DATA_DIR="$HOME/.keyring"

say()  { printf '%s\n' "$*"; }
warn() { printf '⚠️  %s\n' "$*" >&2; }
die()  { printf '❌ %s\n' "$*" >&2; exit 1; }

detect_target() {
  local os arch
  os=$(uname -s)
  arch=$(uname -m)
  case "$os-$arch" in
    Darwin-arm64)  echo "aarch64-apple-darwin" ;;
    Darwin-x86_64) echo "x86_64-apple-darwin" ;;
    Linux-x86_64)  echo "x86_64-unknown-linux-gnu" ;;
    Linux-aarch64) echo "aarch64-unknown-linux-gnu" ;;
    *) die "没有 $os-$arch 的预编译二进制。装好 Rust 后用 cargo install --path rust 自行编译。" ;;
  esac
}

# 清掉 Python 版的入口。只删「入口」，数据目录里的密钥文件一个都不动。
cleanup_python_version() {
  local cleaned=0

  # 1) pipx 装的
  if command -v pipx >/dev/null 2>&1 && pipx list --short 2>/dev/null | grep -q '^kyvault '; then
    say "🧹 卸载 pipx 装的 Python 版…"
    pipx uninstall kyvault >/dev/null 2>&1 || warn "pipx uninstall 没成功，可稍后手动执行"
    cleaned=1
  fi

  # 2) 旧 install.sh 在 /usr/local/bin 放的 python shim。
  #    先确认它真的是 python 脚本再删 —— 万一那里已经是 Rust 二进制（重复执行本脚本），
  #    删掉就把刚装好的给删了。
  local legacy="/usr/local/bin/$BIN_NAME"
  if [ -f "$legacy" ] && head -c 200 "$legacy" 2>/dev/null | grep -q "python"; then
    say "🧹 移除旧的 Python shim：$legacy"
    if rm -f "$legacy" 2>/dev/null; then :; else
      sudo rm -f "$legacy" 2>/dev/null || warn "删不掉 $legacy，请手动 sudo rm -f $legacy"
    fi
    cleaned=1
  fi

  # 3) 旧 install.sh 复制进数据目录的模块副本。
  #    只删 module 这一个子目录，master.key / secrets.json / aliases.json 保持原样。
  if [ -d "$DATA_DIR/module/kyvault" ]; then
    say "🧹 移除旧的模块副本：$DATA_DIR/module"
    rm -rf "$DATA_DIR/module"
    cleaned=1
  fi

  [ "$cleaned" = 1 ] && say "   （密钥数据未动：$DATA_DIR 下的 master.key / secrets.json / aliases.json 原样保留）"
  return 0
}

install_from_release() {
  local target url tmp
  target=$(detect_target)
  url="https://github.com/$REPO/releases/latest/download/${BIN_NAME}-${target}.tar.gz"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' RETURN

  say "📦 下载 $target …"
  if ! curl -fsSL "$url" -o "$tmp/pkg.tar.gz"; then
    warn "下载失败：$url"
    return 1
  fi
  tar -xzf "$tmp/pkg.tar.gz" -C "$tmp"
  local found
  found=$(find "$tmp" -type f -name "$BIN_NAME" -perm -u+x | head -1)
  [ -n "$found" ] || { warn "压缩包里找不到 $BIN_NAME"; return 1; }
  mkdir -p "$INSTALL_DIR"
  install -m 0755 "$found" "$INSTALL_DIR/$BIN_NAME"
  return 0
}

install_from_source() {
  command -v cargo >/dev/null 2>&1 || return 1
  local here
  here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  [ -f "$here/rust/Cargo.toml" ] || return 1
  say "🔨 用本地源码编译（cargo）…"
  cargo install --path "$here/rust" --root "${INSTALL_DIR%/bin}" --force >/dev/null
  return 0
}

say "🔐 kyvault (Rust) — 安装中…"
cleanup_python_version

if ! install_from_release; then
  say "改用源码编译兜底…"
  install_from_source || die "预编译二进制下载失败，且本机没有 cargo + 源码可兜底"
fi

BIN="$INSTALL_DIR/$BIN_NAME"
[ -x "$BIN" ] || die "装完却找不到可执行文件：$BIN"

say ""
say "✅ 安装完成：$($BIN --version)"
say "   位置：$BIN"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) warn "$INSTALL_DIR 不在 PATH 里，把这行加进 shell 配置：
     export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac

if [ -f "$DATA_DIR/secrets.json" ] || [ -f "$DATA_DIR/master.key" ]; then
  say ""
  say "检测到已有密钥库，两版格式完全兼容，直接用即可："
  say "  kyvault list"
else
  say ""
  say "首次使用："
  say "  kyvault init                       # 生成 master key"
  say "  kyvault set secret://github/pat -  # 从 stdin 写入，密钥不进进程参数"
  say "  kyvault list                       # 只出元信息，不出明文"
  say "  kyvault run --env T=secret://github/pat -- your-command"
fi
