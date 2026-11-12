//! Agent 录入请求的端到端行为：声明 → 落地 → 回执，以及失败路径不落库。
//!
//! 这些测试跑在 `KYVAULT_STORE_DIR` 指定的隔离目录里，绝不碰真实密钥库。

use kyvault::alias::Aliases;
use kyvault::requests::{self, Spec, Status};

fn fixture_store() -> (tempfile::TempDir, kyvault::store::Store, Aliases) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(".keyring");
    std::fs::create_dir_all(&dir).unwrap();
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for f in ["master.key", "secrets.json"] {
        std::fs::copy(base.join(f), dir.join(f)).unwrap();
    }
    let store = kyvault::store::Store::new(&dir);
    let aliases = Aliases::new(&dir);
    (tmp, store, aliases)
}

fn with_inbox<F: FnOnce() -> R, R>(f: F) -> R {
    // requests::inbox_dir() 跟 store::default_store_dir() 同根；
    // 用 KYVAULT_STORE_DIR 指到独立临时目录。注意：env 是进程级的，
    // 依赖它的测试必须串行跑（见下面的 serial 声明）。
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("KYVAULT_STORE_DIR", tmp.path());
    let out = f();
    std::env::remove_var("KYVAULT_STORE_DIR");
    out
}

fn spec(platform: &str, name: &str) -> Spec {
    Spec {
        platform: platform.into(),
        name: name.into(),
        account: "octocat".into(),
        kind: String::new(),
        alias: "github_token".into(),
        note: "CI 用".into(),
    }
}

fn step_declare_creates_pending_request_with_meta_only() {
    with_inbox(|| {
        let req = requests::declare(spec("github", "ci-token"), "test-agent", None).unwrap();
        assert_eq!(req.status, Status::Pending);
        assert_eq!(req.spec.platform, "github");
        assert!(!req.id.is_empty());

        // 落盘文件里不允许出现「密文」这个词级别的字段
        let body = std::fs::read_to_string(
            requests::inbox_dir()
                .unwrap()
                .join(format!("{}.json", req.id)),
        )
        .unwrap();
        assert!(!body.contains("secret_value"));
        assert!(!body.contains("ciphertext"));

        let pending = requests::pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, req.id);
    });
}

fn step_declare_rejects_invalid_names() {
    with_inbox(|| {
        let mut bad = spec("github", "../escape");
        bad.name = "../escape".into();
        assert!(requests::declare(bad, "test", None).is_err());

        let mut empty = spec("", "x");
        empty.platform = String::new();
        assert!(requests::declare(empty, "test", None).is_err());
    });
}

fn step_unknown_platform_saves_unverified() {
    with_inbox(|| {
        let (_tmp, store, aliases) = fixture_store();
        let req = requests::declare(spec("unknownplatform", "tok"), "t", None).unwrap();
        let outcome = requests::fulfill(&store, &aliases, &req.id, "  some-value-1234  ").unwrap();
        assert_eq!(outcome.status, Status::Done);
        assert_eq!(
            store
                .get_secret("secret://unknownplatform/tok")
                .unwrap()
                .as_deref(),
            // fulfill 里 trim 过
            Some("some-value-1234")
        );
        assert_eq!(
            aliases.get("github_token").as_deref(),
            Some("secret://unknownplatform/tok")
        );
        assert_eq!(outcome.last4.as_deref(), Some("1234"));
    });
}

fn step_failed_request_rejects_double_fulfill() {
    with_inbox(|| {
        let (_tmp, store, aliases) = fixture_store();
        let req = requests::declare(spec("unknownplatform", "one"), "t", None).unwrap();
        requests::fulfill(&store, &aliases, &req.id, "value-one").unwrap();
        // 同一条请求不能二次入库
        assert!(requests::fulfill(&store, &aliases, &req.id, "value-two").is_err());
    });
}

fn step_abort_marks_failed() {
    with_inbox(|| {
        let req = requests::declare(spec("github", "gone"), "t", None).unwrap();
        requests::fail(&req.id, "已被 agent 或用户放弃").unwrap();
        assert_eq!(requests::get(&req.id).unwrap().status, Status::Failed);
    });
}

fn step_clean_removes_finished_but_keeps_pending() {
    with_inbox(|| {
        let (_tmp, store, aliases) = fixture_store();
        let done = requests::declare(spec("unknownplatform", "d"), "t", None).unwrap();
        requests::fulfill(&store, &aliases, &done.id, "v").unwrap();
        let keep = requests::declare(spec("github", "keep"), "t", None).unwrap();
        requests::fail(&done.id, "x").ok(); // 已是 done，应失败但不影响 clean
        let _ = keep;

        let mut removed = 0;
        for r in requests::list().unwrap() {
            if r.status != Status::Pending && requests::remove(&r.id).unwrap() {
                removed += 1;
            }
        }
        assert_eq!(removed, 1);
        let left = requests::list().unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].status, Status::Pending);
    });
}

fn step_xiaomi_token_plan_saves_without_probe() {
    with_inbox(|| {
        let (_tmp, store, aliases) = fixture_store();
        let req = requests::declare(spec("xiaomi", "token-plan"), "test-agent", None).unwrap();
        let outcome =
            requests::fulfill(&store, &aliases, &req.id, "synthetic-xiaomi-key-1234").unwrap();
        assert_eq!(outcome.status, Status::Done);
        assert!(outcome.message.contains("不做自动探测"));
        assert_eq!(
            store
                .get_secret("secret://xiaomi/token-plan")
                .unwrap()
                .as_deref(),
            Some("synthetic-xiaomi-key-1234")
        );
        assert_eq!(
            aliases.get("github_token").as_deref(),
            Some("secret://xiaomi/token-plan")
        );
        let receipt = requests::get(&req.id).unwrap().result.unwrap();
        assert_eq!(receipt.status, "unverified");
        assert!(receipt.message.contains("不做自动探测"));
    });
}

/// 这些测试共享进程级 KYVAULT_STORE_DIR，必须串行 —— 合成一个入口按序跑。
#[test]
fn requests_flow_serial() {
    step_declare_creates_pending_request_with_meta_only();
    step_declare_rejects_invalid_names();
    step_unknown_platform_saves_unverified();
    step_xiaomi_token_plan_saves_without_probe();
    step_failed_request_rejects_double_fulfill();
    step_abort_marks_failed();
    step_clean_removes_finished_but_keeps_pending();
}
