//! 密文文件权限回归测试 —— 钉的是「git 同步之后」这个真实场景。
//!
//! 2026-10-12 实测（南烛，`~/.config/kyvault/store`）：
//!
//! ```text
//! $ stat -c '%a %n' secrets.json meta.json master.key.enc auth.json
//! 664 secrets.json     <- doctor 报 FAIL
//! 664 meta.json        <- doctor 报 FAIL
//! 600 master.key.enc   <- 从未进过 git
//! 600 auth.json        <- 从未进过 git
//! ```
//!
//! ## 根因不是「忘了 chmod」
//!
//! `save()`（store.rs:252）与 `save_meta()`（159）**自己写出来的文件一定是 600**
//! —— 都走「写 tmp -> harden -> rename」。实测新建库：secrets.json=600、
//! master.key=600。所以「某次忘了 chmod」不是根因。
//!
//! 真正的来源在别处：数据目录本身是 **git 仓库**（GitLab 团队密钥库）。
//! **git 不保存文件权限** —— `git checkout` / `git pull` 落盘时按当前 umask
//! 建文件（默认 002 -> 664），哪怕仓里的 blob 记录的是 600。所以每次同步密钥库，
//! 密文文件权限就被打回 664，手动 chmod 下次 sync 又复发。
//!
//! 修法不放在「记得 chmod」这种要人记得的地方，而放在**每次接触库之前**。

#![cfg(unix)]

use kyvault::store::{harden, Store};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn assert_600(path: &Path) {
    let mode = fs::metadata(path).expect("读不到文件").permissions().mode() & 0o777;
    assert_eq!(
        mode,
        0o600,
        "{} 权限是 {:o}，应为 600 —— 密文组可读违背最小权限",
        path.display(),
        mode
    );
}

fn widen(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o664)).unwrap();
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o664
    );
}

/// 建一个两种密文文件都落盘的库。
/// meta.json 走显式 save_meta：set_secret 只写密文，annotate/save_meta 才写 meta。
fn seeded_store() -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().expect("tempdir");
    let s = Store::new(d.path());
    s.init_master_key().expect("init master key");
    s.set_secret("secret://t/a", "v").expect("写密文");
    s.save_meta(&serde_json::Map::new()).expect("写 meta");
    assert!(s.secrets_path().exists(), "secrets.json 没生成");
    assert!(s.meta_file().exists(), "meta.json 没生成");
    (d, s)
}

#[test]
fn harden_tightens_a_wide_file_to_600() {
    let d = tempfile::tempdir().expect("tempdir");
    let f = d.path().join("x.json");
    fs::write(&f, b"{}").unwrap();
    widen(&f);
    harden(&f).expect("harden 失败");
    assert_600(&f);
}

/// 基线：kyvault **自己写**出来的密文文件就是 600。
/// 有了这条，才能把锅准确甩给 git，而不是 kyvault。
#[test]
fn kyvault_own_writes_are_600() {
    let (_d, s) = seeded_store();
    assert_600(&s.secrets_path());
    assert_600(&s.meta_file());
}

/// 核心回归：模拟 `git pull` 把权限打回 664，收紧必须能救回来。
#[test]
fn widen_after_sync_can_be_restored() {
    let (_d, s) = seeded_store();
    widen(&s.secrets_path());
    widen(&s.meta_file());

    s.repair_perms().expect("repair_perms 失败");

    assert_600(&s.secrets_path());
    assert_600(&s.meta_file());
}

/// 幂等，且不能在下一次读写后掉回去。
#[test]
fn repair_is_idempotent_and_survives_rewrite() {
    let (_d, s) = seeded_store();
    s.repair_perms().expect("第一次");
    s.repair_perms().expect("第二次（幂等）");
    assert_600(&s.secrets_path());
    assert_600(&s.meta_file());

    widen(&s.secrets_path());
    s.set_secret("secret://t/b", "w").expect("再写一条");
    assert_600(&s.secrets_path());
}

/// 没初始化过的目录不能报错 —— repair 是「顺手收紧」，不是「必须可用」。
#[test]
fn repair_on_empty_dir_is_noop() {
    let d = tempfile::tempdir().expect("tempdir");
    let s = Store::new(d.path());
    s.repair_perms().expect("空目录 repair 不该失败");
}
