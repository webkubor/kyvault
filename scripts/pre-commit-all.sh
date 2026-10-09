#!/bin/bash
# kyvault 仓库级 pre-commit 门禁。
#
# 由全局 ~/.config/git/hooks/pre-commit 调用（core.hooksPath 指向全局）——
# 所以仓库自己的 hooks/ 目录不会被 git 看到，门禁必须放这里。
# 全局钩子的约定：脚本不存在 = 这个仓没有门禁，放过。所以「建这个文件」
# 本身就是启用门禁的那一步。
#
# ## 为什么要有这个
#
# 2026-10-08：main 的 CI 从 10-05 起一直红着，卡在 `cargo fmt --check`
# （main.rs 3 处 + vaultrepo.rs 3 处未格式化，零语义改动）。它把后续两个 PR
# 全挡住了 —— 包括完全不碰 Rust 的那个（#7），因为 fmt 卡的是整个 crate，
# 不管你改没改 Rust。
#
# 根因不是「有人忘了跑 fmt」这么简单，而是**没有任何机制提醒**：仓库没有
# pre-commit 钩子，CONTRIBUTING 的提交清单里也没有 fmt 这一条。于是改完
# Rust 直接 commit，一路畅通到 main 才被 CI 拦下，而 main 的 CI 又是红的，
# 于是连"红了"这件事本身也没人当回事。
#
# ## 为什么只查 fmt
#
# 约定是「commit 时只快检，重活挪 CI」（见全局钩子注释与 CortexOS 的
# cs flow cortexos ⑤）。这里的划分：
#   · cargo fmt --check   —— 亚秒级，纯文本比对，**放这里**
#   · cargo clippy        —— 首次全量编译要几十秒 → CI
#   · cargo test          —— 更慢 → CI
#
# fmt 是唯一一个「零成本却最容易漂」的：clippy 抓不到它，test 更抓不到，
# 而它一旦漂移就把整条 CI 卡死。
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# 没暂存 Rust 改动就不跑 —— rustfmt 要读整个 crate，跑它得先确认这仓
# 真的有 Rust 源码（kyvault 主体是 rust/，但仓库里也有 .py/.sh 工具）。
if ! git diff --cached --name-only | grep -qE '\.rs$'; then
  exit 0
fi

if [ ! -d rust ]; then
  echo "⚠️ 暂存了 .rs 改动，但仓库里没有 rust/ 目录 —— 跳过 fmt 检查"
  exit 0
fi

command -v cargo >/dev/null || {
  echo "⚠️ 没装 cargo，跳过 fmt 检查（CI 里还会再查一遍）"
  exit 0
}

cd rust
if ! cargo fmt --check 2>/tmp/ky-fmt-diff.$$; then
  cat /tmp/ky-fmt-diff.$$
  rm -f /tmp/ky-fmt-diff.$$
  cat >&2 <<'EOF'

✗ Rust 代码没跑 cargo fmt。修一下再提交：

    cd rust && cargo fmt

不想让 CI 来发现这个问题，就装上这个门禁（一次性）：
    chmod +x scripts/pre-commit-all.sh   # 本文件需可执行
全局钩子已经会自动调用它 —— 前提是 core.hooksPath 指向全局（默认如此）。

没装 cargo / 不想现在格式化，也可以用 --no-verify 跳过本次：
    git commit --no-verify
EOF
  exit 1
fi
rm -f /tmp/ky-fmt-diff.$$

echo "✓ pre-commit: cargo fmt 干净"