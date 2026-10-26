//! CLI security boundaries exercised with temporary stores and public test keys.
use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn ky(root: &std::path::Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kyvault"));
    command
        .args(args)
        .env("KYVAULT_STORE_DIR", root)
        .env("KYVAULT_BACKEND", "file")
        .env_remove("KEYRING_MASTER_KEY")
        .env_remove("KYVAULT_ALLOW_INSECURE_LEGACY_AUTH")
        .env_remove("SSH_ORIGINAL_COMMAND");
    command
}
fn send(mut command: Command, input: &str) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn legacy_setup_is_blocked_before_creating_any_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("not-created");
    let out = ky(&root, &["auth", "setup"]).output().unwrap();
    assert!(!out.status.success());
    assert!(!root.exists());
    assert!(String::from_utf8_lossy(&out.stderr).contains("默认禁止"));
    let out = ky(&root, &["auth", "add", "ssh", "unused.pub"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!root.exists());
}

#[test]
fn broker_cli_protocol_never_exposes_ungranted_or_master_keys() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    assert!(ky(root, &["init", "--no-git"])
        .output()
        .unwrap()
        .status
        .success());
    assert!(
        ky(root, &["set", "secret://test/allowed", "dummy-test-value"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        ky(root, &["set", "secret://test/other", "never-return-this"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let public = root.join("device.pub");
    fs::write(
        &public,
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOv3Q1J3Wompk6Ke5fKU8bQrjYAlBlG/kySLh8vh8J3O\n",
    )
    .unwrap();
    let out = ky(
        root,
        &[
            "broker",
            "register",
            "agent-test",
            "--principal",
            "agent:deployment",
            "--pubkey",
            public.to_str().unwrap(),
            "--allow-ip",
            "192.0.2.0/24",
            "--allow-ref",
            "secret://test/allowed",
            "--ttl-hours",
            "1",
        ],
    )
    .output()
    .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let args = [
        "broker",
        "serve",
        "--store",
        root.to_str().unwrap(),
        "--device",
        "agent-test",
    ];
    let mut command = ky(root, &args);
    command.env("SSH_CONNECTION", "192.0.2.7 1234 192.0.2.1 22");
    let out = send(
        command,
        r#"{"version":1,"operation":"inject","refs":["secret://test/allowed"]}"#,
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let response: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        response["values"]["secret://test/allowed"],
        "dummy-test-value"
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let master = fs::read_to_string(root.join("master.key")).unwrap();
    assert!(!text.contains(master.trim()));
    assert!(!text.contains("never-return-this"));
    for (connection, input) in [
        (
            "192.0.2.7 1234 192.0.2.1 22",
            r#"{"version":1,"operation":"inject","refs":["secret://test/other"]}"#,
        ),
        (
            "198.51.100.1 1234 192.0.2.1 22",
            r#"{"version":1,"operation":"inject","refs":["secret://test/allowed"]}"#,
        ),
        (
            "192.0.2.7 1234 192.0.2.1 22",
            "not-json-dummy-private-input",
        ),
    ] {
        let mut command = ky(root, &args);
        command.env("SSH_CONNECTION", connection);
        let out = send(command, input);
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
    }
    let key_line = ky(root, &["broker", "authorized-key", "agent-test"])
        .output()
        .unwrap();
    assert!(key_line.status.success());
    let key_line = String::from_utf8(key_line.stdout).unwrap();
    assert!(key_line.contains("restrict,from=\"192.0.2.0/24\",command="));
    assert!(!key_line.contains(master.trim()));
    assert!(ky(root, &["broker", "revoke", "agent-test"])
        .output()
        .unwrap()
        .status
        .success());
    let mut command = ky(root, &args);
    command.env("SSH_CONNECTION", "192.0.2.7 1234 192.0.2.1 22");
    let out = send(
        command,
        r#"{"version":1,"operation":"inject","refs":["secret://test/allowed"]}"#,
    );
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    let audit = fs::read_to_string(root.join("broker-audit.jsonl")).unwrap();
    for value in [
        "dummy-test-value",
        "never-return-this",
        master.trim(),
        "not-json-dummy-private-input",
    ] {
        assert!(!audit.contains(value));
    }
    assert!(audit.contains("request-rejected"));
    assert!(audit.contains("deny"));
    let status = ky(root, &["broker", "status"]).output().unwrap();
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["devices"][0]["state"], "revoked");
}

#[test]
fn broker_grant_cannot_reference_whole_server_or_cli() {
    let directory = tempfile::tempdir().unwrap();
    let st = kyvault::store::Store::new(directory.path());
    st.init_master_key().unwrap();
    st.set_secret("secret://server/host/ip", "192.0.2.1")
        .unwrap();
    let d = kyvault::broker::Device {
        id: "test".into(),
        principal: "agent:test".into(),
        public_key: String::new(),
        fingerprint: String::new(),
        approved_by: "local-admin".into(),
        approved_at: String::new(),
        allow_ips: vec!["192.0.2.1".into()],
        refs: vec!["secret://server/host".into()],
        expires_unix: u64::MAX,
        revoked: false,
    };
    let r = kyvault::broker::Request {
        version: 1,
        operation: "inject".into(),
        refs: vec!["secret://server/host".into()],
    };
    assert!(kyvault::broker::authorize(&d, &r, "192.0.2.1".parse().unwrap(), 0).is_err());
}
