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
CLI_TAG_RE='^v(0|[1-9][0-9]*)[.](0|[1-9][0-9]*)[.](0|[1-9][0-9]*)$'

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

# GitHub 的 latest 可能是 desktop-v*。只从 Release 顶层对象读取三个字段，
# 忽略 release note 字符串和嵌套对象；不要求新装机器装 jq、Python 或 Rust。
cli_release_page() {
  LC_ALL=C awk -v tag_re="$CLI_TAG_RE" '
    {
      rest = $0
      while (rest ~ /[^[:space:]]/) {
        sub(/^[[:space:]]+/, "", rest)
        if (!match(rest, /^("([^"\\]|\\.)*"|\[|\]|\{|\}|,|:|[a-zA-Z0-9.+-]+)/)) exit 1
        tok = substr(rest, 1, RLENGTH)
        rest = substr(rest, RLENGTH + 1)
        if (!started) {
          if (tok != "[") exit 1
          started = 1
        } else if (depth == 0) exit 1
        if (depth == 2 && key != "") {
          if (key == "\"tag_name\"") tag = tok ~ /^"/ ? substr(tok, 2, length(tok) - 2) : ""
          if (key == "\"draft\"") draft = tok
          if (key == "\"prerelease\"") prerelease = tok
          key = ""
        } else if (depth == 2 && tok == ":") key = prev
        if (tok == "[" || tok == "{") {
          if (depth == 1 && tok != "{") exit 1
          depth++
          if (depth == 2) { rows++; tag = draft = prerelease = key = "" }
        }
        if (tok == "}" || tok == "]") {
          if (depth == 1 && tok != "]") exit 1
          if (depth == 2 && tok == "}" && draft == "false" && prerelease == "false" && tag ~ tag_re) {
            versions[++n] = substr(tag, 2)
          }
          depth--
        }
        prev = tok
      }
    }
    END {
      if (!started || depth != 0) exit 1
      print rows + 0
      for (i = 1; i <= n; i++) print versions[i]
    }
  '
}

latest_cli_tag() {
  local page=1 result count version
  local versions=""
  while :; do
    result=$(curl -fsSL --connect-timeout 5 --max-time 15 \
      -H 'Accept: application/vnd.github+json' \
      "https://api.github.com/repos/$REPO/releases?per_page=100&page=$page" | cli_release_page) || return 1
    count=${result%%$'\n'*}
    if [ "$result" != "$count" ]; then
      versions="$versions${result#*$'\n'}"$'\n'
    fi
    [ "$count" -lt 100 ] && break
    page=$((page + 1))
  done
  version=$(printf '%s' "$versions" | LC_ALL=C sort -t . -k1,1n -k2,2n -k3,3n | tail -n1)
  [ -n "$version" ] || return 1
  printf 'v%s\n' "$version"
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

activate_binary() (
  local binary=$1 stage
  mkdir -p "$INSTALL_DIR" || return 1
  stage=$(mktemp -d "$INSTALL_DIR/.kyvault-install.XXXXXXXX") || return 1
  trap 'rm -rf "$stage"' EXIT
  install -m 0755 "$binary" "$stage/$BIN_NAME" || return 1
  # 只有已经备好替换二进制才迁移旧入口；pipx 不应看到新装的同名入口。
  cleanup_python_version
  mv -f "$stage/$BIN_NAME" "$INSTALL_DIR/$BIN_NAME"
)

install_from_release() (
  local tag=$1 target url tmp found version
  target=$(detect_target) || return 1
  url="https://github.com/$REPO/releases/download/$tag/${BIN_NAME}-${target}.tar.gz"
  tmp=$(mktemp -d) || return 1
  trap 'rm -rf "$tmp"' EXIT

  say "📦 下载 $tag ($target) …"
  # 先按默认（HTTP/2）下，失败就退 HTTP/1.1 重试一次。
  #
  # 2026-09-10 实测：本机默认 curl 报 `(16) Error in the HTTP2 framing layer`，
  # 同一个 URL 加 --http1.1 立刻成功 —— 这类故障常见于中间有代理/企业网关，
  # 用户看到的现象却是「安装脚本说下载失败」，完全看不出是协议协商的问题。
  # 重试一次几乎零成本，能救掉相当一部分装不上的情况。
  if ! curl -fsSL --retry 2 --retry-delay 1 "$url" -o "$tmp/pkg.tar.gz"; then
    warn "HTTP/2 下载失败，退 HTTP/1.1 重试…"
    if ! curl -fsSL --http1.1 --retry 2 --retry-delay 1 "$url" -o "$tmp/pkg.tar.gz"; then
      warn "下载失败：$url"
      return 1
    fi
  fi
  tar -xzf "$tmp/pkg.tar.gz" -C "$tmp" || { warn "解包失败：$url"; return 1; }
  found=$(find "$tmp" -type f -name "$BIN_NAME" -perm -u+x | head -1) || return 1
  [ -n "$found" ] || { warn "压缩包里找不到 $BIN_NAME"; return 1; }
  version=$("$found" --version) || { warn "无法验证预编译二进制版本"; return 1; }
  [ "$version" = "$BIN_NAME ${tag#v}" ] || { warn "压缩包版本与 $tag 不符"; return 1; }
  activate_binary "$found"
)

install_from_source() (
  command -v cargo >/dev/null 2>&1 || return 1
  local tmp src="" script="${BASH_SOURCE[0]:-}"
  tmp=$(mktemp -d) || return 1
  trap 'rm -rf "$tmp"' EXIT
  if [ -n "$script" ]; then
    src="$(cd "$(dirname "$script")" 2>/dev/null && pwd)" || src=""
  fi
  if [ -n "$src" ] && [ -f "$src/rust/Cargo.toml" ]; then
    say "🔨 用本地源码编译（cargo）…"
  else
    command -v git >/dev/null 2>&1 || return 1
    src="$tmp/source"
    say "🔨 没有本地源码，现拉一份再编译（cargo）…"
    git clone --depth 1 "https://github.com/$REPO.git" "$src" >/dev/null 2>&1 || return 1
  fi
  cargo install --path "$src/rust" --root "$tmp/install" --force >/dev/null || return 1
  activate_binary "$tmp/install/bin/$BIN_NAME"
)

[ "$#" -le 1 ] || die "用法：bash install.sh [vX.Y.Z]"
tag=""
if [ "$#" -eq 1 ]; then
  tag="$1"
  [[ "$tag" =~ $CLI_TAG_RE ]] || die "无效的 CLI 版本标签：需要 vX.Y.Z"
fi
say "🔐 kyvault (Rust) — 安装中…"
if [ "$#" -eq 1 ]; then
  install_from_release "$tag" || die "安装 $tag 失败；未改用其他版本或源码。"
elif ! tag=$(latest_cli_tag) || ! install_from_release "$tag"; then
  [ -n "$tag" ] || warn "无法确定最新稳定 CLI 版本"
  say "改用源码编译兜底…"
  install_from_source || die "装不上：预编译二进制下载失败，源码兜底也没成（需要 cargo + git）。手动装：git clone https://github.com/$REPO && cd kyvault/rust && cargo install --path ."
fi

BIN="$INSTALL_DIR/$BIN_NAME"
[ -x "$BIN" ] || die "装完却找不到可执行文件：$BIN"
ln -sf "$BIN" "$INSTALL_DIR/ky"

say ""
say "✅ 安装完成：$("$BIN" --version)"
say "   命令：$BIN （或快捷短命令 ky）"

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
