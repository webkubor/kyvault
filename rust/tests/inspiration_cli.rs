//! 灵感命令无需初始化密钥库，且不受密钥后端配置影响。
use std::io::Write;
use std::process::{Command, Stdio};

fn command(root: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kyvault"));
    cmd.env("KY_INSPIRATION_DIR", root.join("ideas"))
        .env("KYVAULT_STORE_DIR", root.join("vault-not-initialized"))
        .env("KYVAULT_BACKEND", "invalid-backend")
        .env("KY_AUDIT", "0");
    cmd
}

#[test]
fn idea_workflow_without_a_vault() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let mut child = command(root)
        .args(["idea", "add", "一点想法", "-", "--tags", "Ky,产品"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("第一行\n第二行\n".as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = command(root)
        .args(["inspiration", "ls", "第二行", "--tag", "产品", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let items: Vec<kyvault::inspiration::Idea> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(items.len(), 1);
    let id = &items[0].id;
    assert_eq!(items[0].body, "第一行\n第二行\n");
    let path_output = command(root).args(["idea", "path", id]).output().unwrap();
    assert!(path_output.status.success());
    let path = std::path::PathBuf::from(String::from_utf8(path_output.stdout).unwrap().trim());
    assert_eq!(path.extension().unwrap(), "md");
    assert!(path.starts_with(root.join("ideas").canonicalize().unwrap()));
    let raw = std::fs::read_to_string(path).unwrap();
    assert!(raw.contains("第二行"));
    assert!(!root.join("vault-not-initialized").exists());

    assert!(command(root)
        .args(["idea", "edit", id, "--title", "新的想法", "--tags", ""])
        .status()
        .unwrap()
        .success());
    let output = command(root)
        .args(["idea", "show", id, "--json"])
        .output()
        .unwrap();
    let idea: kyvault::inspiration::Idea = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(idea.title, "新的想法");
    assert!(idea.tags.is_empty());
    assert_eq!(idea.created_at, items[0].created_at);
    assert!(!command(root)
        .args(["idea", "show", "../outside"])
        .output()
        .unwrap()
        .status
        .success());
    assert!(command(root)
        .args(["idea", "rm", id])
        .status()
        .unwrap()
        .success());
    let output = command(root)
        .args(["idea", "list", "--json"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "[]");
    assert!(!root.join("vault-not-initialized").exists());
}

#[test]
fn markdown_capture_external_edits_and_legacy_migration() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let body = "一个念头\n\n- 保留 Markdown  \n- 第二项\n";
    let output = command(root)
        .args([
            "idea",
            "jot",
            body,
            "--source",
            "https://example.com/note",
            "--tags",
            "阅读",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = command(root)
        .args(["idea", "list", "--json"])
        .output()
        .unwrap();
    let items: Vec<kyvault::inspiration::Idea> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "一个念头");
    assert_eq!(items[0].body, body);
    assert_eq!(items[0].source.as_deref(), Some("https://example.com/note"));
    let path = std::path::Path::new(&items[0].path);
    assert_eq!(path.extension().unwrap(), "md");
    let raw = std::fs::read_to_string(path).unwrap();
    assert!(raw.contains(body));
    std::fs::write(path, raw.replace("第二项", "外部编辑器改过")).unwrap();
    let output = command(root)
        .args(["idea", "show", &items[0].id, "--json"])
        .output()
        .unwrap();
    let changed: kyvault::inspiration::Idea = serde_json::from_slice(&output.stdout).unwrap();
    assert!(changed.body.contains("外部编辑器改过"));
    assert_eq!(changed.id, items[0].id);

    let legacy = root.join("legacy");
    std::fs::create_dir(&legacy).unwrap();
    let old = serde_json::json!({"id":"idea-legacy", "title":"以前的灵感", "body":"保留原件", "tags":["旧笔记"], "created_at":"2026-11-01T00:00:00Z", "updated_at":"2026-11-01T00:00:00Z"});
    let old_file = legacy.join("idea-legacy.json");
    let old_bytes = serde_json::to_vec_pretty(&old).unwrap();
    std::fs::write(&old_file, &old_bytes).unwrap();
    for _ in 0..2 {
        let output = command(root)
            .args(["idea", "migrate", "--from"])
            .arg(&legacy)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert_eq!(std::fs::read(&old_file).unwrap(), old_bytes);
    let output = command(root)
        .args(["idea", "list", "--json"])
        .output()
        .unwrap();
    let items: Vec<kyvault::inspiration::Idea> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items.iter().filter(|i| i.title == "以前的灵感").count(), 1);
    assert!(!root.join("vault-not-initialized").exists());
}
