//! 灵感命令无需初始化密钥库，且不受密钥后端配置影响。
use std::io::Write;
use std::process::{Command, Stdio};

fn command(root: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_kyvault"));
    cmd.env("KY_INSPIRATION_DIR", root.join("ideas"))
        .env("KYVAULT_STORE_DIR", root.join("vault-not-initialized"))
        .env("KYVAULT_BACKEND", "invalid-backend")
        .env("HOME", root)
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
    assert_eq!(items[0].body, "第一行\n第二行");
    let raw = std::fs::read_to_string(root.join("ideas").join(format!("{id}.json"))).unwrap();
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
