# 🤝 Contributing

Thanks for your interest in contributing!

## How to Contribute

1. **Fork** this repository
2. **Create a branch** (`git checkout -b feat/amazing-feature`)
3. **Commit your changes** (`git commit -m 'feat: add amazing feature'`)
4. **Push** (`git push origin feat/amazing-feature`)
5. **Open a Pull Request**

### Rust: 先跑 `cargo fmt`

```bash
cd rust && cargo fmt
```

提交钩子 `scripts/pre-commit-all.sh`（由全局 `~/.config/git/hooks/pre-commit`
调用）会在你暂存了 `.rs` 改动时跑 `cargo fmt --check`，不过就拦下。

> 2026-10-08 加这条的原因：main 的 CI 从 10-05 起一直红在 `cargo fmt --check`
> ——`main.rs` 3 处 + `vaultrepo.rs` 3 处，全是超长数组字面量没拆行，**零语义改动**。
> 它把后续两个 PR 全挡住了，包括**完全不碰 Rust** 的那个：fmt 卡的是整个 crate，
> 跟你改没改 Rust 无关。
>
> 紧急情况可以 `git commit --no-verify` 跳过，但 CI 还会再拦一次。

门禁只做快检（亚秒级）。`cargo clippy` 和 `cargo test` 留在 CI —— 它们要先编译，
放进 commit 会让人等几十秒，等久了就会习惯性 `--no-verify`，门禁也就白设了。

## Development Setup

```bash
git clone https://github.com/webkubor/agent-secret-skills.git
cd agent-secret-skills
# Follow the Quick Start in README
```

## Commit Convention

We follow [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` New feature
- `fix:` Bug fix
- `docs:` Documentation
- `refactor:` Code refactoring
- `chore:` Maintenance

## Questions?

Open an issue or discussion — we respond within 24 hours.
