//! 跨语言互操作 —— 这次重写唯一不能出错的地方。
//!
//! 现网有 141 条密钥躺在 Cloudflare D1 里，本地 file 后端另有存量，
//! 全部由 Python 版（和 CortexOS 的 Go 端）写入。Rust 版如果解不开，
//! 不是「功能少一点」，是**用户的密钥库直接打不开**。
//!
//! 所以这里不测 Rust 自己的 roundtrip（那在 crypto.rs 的单测里），
//! 只测一件事：**Python 加密的 Rust 能解，Rust 加密的 Python 能解**，
//! 两套 wire format 各测一遍。
//!
//! 没有可用的 python3 + cryptography 时跳过而不是失败：CI 容器里不该
//! 为了跑这个测试装一套 Python，本机开发时它必须真的跑。

use std::process::Command;

use kyvault::crypto::{
    d1_key, decrypt_joined, decrypt_split, derive_file_key, encrypt_joined, encrypt_split,
    new_master_key_b64,
};

fn python_ready() -> bool {
    Command::new("python3")
        .args(["-c", "import cryptography"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

const PY_JOINED_ENC: &str = r#"
import base64, hashlib, os, sys
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
master, plaintext = sys.argv[1], sys.argv[2]
key = hashlib.sha256(base64.b64decode(master)).digest()
nonce = os.urandom(12)
ct = AESGCM(key).encrypt(nonce, plaintext.encode(), b"")
print(base64.b64encode(nonce + ct).decode())
"#;

const PY_JOINED_DEC: &str = r#"
import base64, hashlib, sys
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
master, blob = sys.argv[1], sys.argv[2]
key = hashlib.sha256(base64.b64decode(master)).digest()
data = base64.b64decode(blob)
print(AESGCM(key).decrypt(data[:12], data[12:], b"").decode())
"#;

const PY_SPLIT_ENC: &str = r#"
import base64, os, sys
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
master, plaintext = sys.argv[1], sys.argv[2]
key = base64.b64decode(master)
nonce = os.urandom(12)
ct = AESGCM(key).encrypt(nonce, plaintext.encode(), None)
print(base64.b64encode(ct).decode() + " " + base64.b64encode(nonce).decode())
"#;

const PY_SPLIT_DEC: &str = r#"
import base64, sys
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
master, ct_b64, nonce_b64 = sys.argv[1], sys.argv[2], sys.argv[3]
key = base64.b64decode(master)
print(AESGCM(key).decrypt(base64.b64decode(nonce_b64), base64.b64decode(ct_b64), None).decode())
"#;

fn py_args(script: &str, args: &[&str]) -> String {
    let mut cmd = Command::new("python3");
    // python3 -c 已经把 argv[0] 占成 "-c"，这里不要再塞占位参数，
    // 否则 argv[1] 拿到的是占位符、真参数整体后移一位（第一版就这么错的）
    cmd.arg("-c").arg(script);
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.output().expect("python3 跑不起来");
    assert!(
        out.status.success(),
        "python 脚本失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// 明文里放中文、emoji 和 base64 里会出现的符号，把 UTF-8 与编码边界一起覆盖
const SAMPLE: &str = "sk-测试密钥/+=abc 🌸 line";

#[test]
fn file_backend_python_encrypt_rust_decrypt() {
    if !python_ready() {
        eprintln!("跳过：没有 python3 + cryptography");
        return;
    }
    let master = new_master_key_b64();
    let blob = py_args(PY_JOINED_ENC, &[&master, SAMPLE]);
    let key = derive_file_key(&master).unwrap();
    assert_eq!(
        decrypt_joined(&blob, &key).unwrap(),
        SAMPLE,
        "Rust 解不开 Python 写的 file 后端密文 —— 现有 ~/.keyring 会打不开"
    );
}

#[test]
fn file_backend_rust_encrypt_python_decrypt() {
    if !python_ready() {
        eprintln!("跳过：没有 python3 + cryptography");
        return;
    }
    let master = new_master_key_b64();
    let key = derive_file_key(&master).unwrap();
    let blob = encrypt_joined(SAMPLE, &key).unwrap();
    assert_eq!(
        py_args(PY_JOINED_DEC, &[&master, &blob]),
        SAMPLE,
        "Python 解不开 Rust 写的密文 —— 两版并存期间会写坏库"
    );
}

#[test]
fn d1_backend_python_encrypt_rust_decrypt() {
    if !python_ready() {
        eprintln!("跳过：没有 python3 + cryptography");
        return;
    }
    let master = new_master_key_b64(); // D1 的 master 就是 base64(32B)，不再派生
    let out = py_args(PY_SPLIT_ENC, &[&master, SAMPLE]);
    let (ct, nonce) = out.split_once(' ').expect("python 该输出 ct 和 nonce 两段");
    let key = d1_key(&master).unwrap();
    assert_eq!(
        decrypt_split(ct, nonce, &key).unwrap(),
        SAMPLE,
        "Rust 解不开 Python 写的 D1 密文 —— 线上 141 条密钥会读不出来"
    );
}

#[test]
fn d1_backend_rust_encrypt_python_decrypt() {
    if !python_ready() {
        eprintln!("跳过：没有 python3 + cryptography");
        return;
    }
    let master = new_master_key_b64();
    let key = d1_key(&master).unwrap();
    let (ct, nonce) = encrypt_split(SAMPLE, &key).unwrap();
    assert_eq!(
        py_args(PY_SPLIT_DEC, &[&master, &ct, &nonce]),
        SAMPLE,
        "Python/Go 解不开 Rust 写的 D1 密文 —— cs kyvault 会读不出新写的密钥"
    );
}

/// 两套格式绝不能互换：拿 file 的派生 key 去解 D1 密文必须失败。
/// 这条防的是「哪天有人图省事把两个 key 函数合并成一个」。
#[test]
fn formats_are_not_interchangeable() {
    let master = new_master_key_b64();
    let file_key = derive_file_key(&master).unwrap();
    let d1 = d1_key(&master).unwrap();
    let (ct, nonce) = encrypt_split(SAMPLE, &d1).unwrap();
    assert!(
        decrypt_split(&ct, &nonce, &file_key).is_err(),
        "file 后端的 key 竟然解开了 D1 密文，说明派生逻辑被写成一样了"
    );
}
