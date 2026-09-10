# 互操作测试向量（KAT）

`master.key` + `secrets.json` 是 **Python 版 kyvault 1.x 真实写出来的一份密钥库**，
用来锁定「Rust 版必须能打开老库」这个契约。

## 为什么要固化成文件

之前 `interop.rs` 里验证 `_servers` / `_clis` 嵌套结构兼容的那条测试，靠现场
`import kyvault.store` 拿 Python 实现当对照 —— 于是它同时依赖：
① 本仓还留着 Python 实现；② 本机装了 `cryptography`。
第一条把「删掉已被 Rust 全覆盖的 Python 实现」这件事永久卡住；第二条让它在 CI
里静默跳过（`python_ready()` 为假就 return），**看着是绿的，其实没跑**。

固化之后：不依赖 Python 实现、不依赖任何 Python 包，CI 里必然真跑。

## 里面是什么（明文都写在测试断言里，见 interop.rs）

| 位置 | 明文 |
|---|---|
| `github/keys/pat` | `ghp_fixture_value_001` |
| `_servers/fx-host` | ip `10.0.0.1` · root-password `fxpass` · cost `42` · provider `tencent` |
| `_clis/fx-cli/main` | `fx_token_777` |

## ⚠️ 这把 master.key 不是密钥

它只能解开同目录这份 `secrets.json`，而里面每一条的明文就写在上面的表里和测试
断言里 —— 公开它零损失。**别把它当成"仓库里泄露了一把密钥"**，也别拿它去解别的库。

## 要重新生成（比如故意扩充覆盖面）

```bash
D=$(mktemp -d)
HOME="$D" PYTHONPATH=<仓库根>:<cryptography 所在 site-packages> python3 -c "
from kyvault.store import init_master_key, set_secret, set_server, set_cli_token
init_master_key()
set_secret('secret://github/pat','ghp_fixture_value_001')
set_server('fx-host','10.0.0.1','fxpass','42','tencent')
set_cli_token('fx-cli','main','fx_token_777')"
cp "$D/.keyring"/{master.key,secrets.json} rust/tests/fixtures/
```

Python 实现删掉之后就没法这样再生成了 —— 那时要扩充，就用 Rust 自己写一份、
再由**当时**的 Go 端（CortexOS `pkg/infra/secretvault`）交叉验证。
