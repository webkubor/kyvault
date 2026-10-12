//! Exercise the actual CLI/Git boundary using only temporary local repositories.
//! No real vault, credentials, user Git hooks, network, or process-wide umask changes.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fixture {
    _temp: tempfile::TempDir,
    home: PathBuf,
    writer: PathBuf,
    remote: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        let writer = temp.path().join("writer");
        let remote = temp.path().join("remote.git");
        let target = temp.path().join("vault");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&writer).unwrap();
        let f = Self {
            _temp: temp,
            home,
            writer,
            remote,
            target,
        };
        f.git(&f.writer, &["init", "-b", "main"]);
        fs::write(f.writer.join(".gitignore"), "master.key\n.lock\n*.tmp\n").unwrap();
        f.commit("{}\n", "{}\n");
        f.git(
            &f.home,
            &[
                "clone",
                "--bare",
                f.writer.to_str().unwrap(),
                f.remote.to_str().unwrap(),
            ],
        );
        f.git(
            &f.writer,
            &["remote", "add", "origin", f.remote.to_str().unwrap()],
        );
        f
    }

    fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        cmd.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", &self.home)
            .env("KYVAULT_STORE_DIR", &self.target)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_COUNT", "2")
            .env("GIT_CONFIG_KEY_0", "core.hooksPath")
            .env("GIT_CONFIG_VALUE_0", "/dev/null")
            .env("GIT_CONFIG_KEY_1", "commit.gpgsign")
            .env("GIT_CONFIG_VALUE_1", "false")
            .env("GIT_AUTHOR_NAME", "Permission Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Permission Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid");
        cmd
    }

    fn git(&self, dir: &Path, args: &[&str]) {
        let output = self
            .command("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        success(&output);
    }

    fn cli(&self, args: &[&str]) -> Output {
        // umask is child-local: Rust tests may run concurrently.
        self.command("/bin/sh")
            .current_dir(&self.home)
            .args(["-c", "umask 002; exec \"$@\"", "permission-test"])
            .arg(env!("CARGO_BIN_EXE_kyvault"))
            .args(args)
            .output()
            .unwrap()
    }

    fn setup(&self) -> Output {
        self.cli(&[
            "gitlab",
            "setup",
            self.remote.to_str().unwrap(),
            "--dir",
            self.target.to_str().unwrap(),
        ])
    }

    fn commit(&self, secrets: &str, meta: &str) {
        fs::write(self.writer.join("secrets.json"), secrets).unwrap();
        fs::write(self.writer.join("meta.json"), meta).unwrap();
        self.git(
            &self.writer,
            &["add", "secrets.json", "meta.json", ".gitignore"],
        );
        self.git(&self.writer, &["commit", "-m", "test fixture"]);
    }

    fn publish(&self, secrets: &str) {
        self.commit(secrets, "{\"updated\": true}\n");
        self.git(&self.writer, &["push", "origin", "main"]);
    }

    fn assert_private(&self) {
        for name in ["secrets.json", "meta.json"] {
            assert_eq!(mode(&self.target.join(name)), 0o600, "{name}");
        }
        assert!(!self.target.join("master.key").exists());
    }
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn setup_hardens_both_files_and_is_idempotent() {
    let f = Fixture::new();
    success(&f.setup());
    f.assert_private();
    for name in ["secrets.json", "meta.json"] {
        fs::set_permissions(f.target.join(name), fs::Permissions::from_mode(0o664)).unwrap();
    }
    success(&f.setup());
    f.assert_private();
}

#[test]
fn pull_hardens_files_rewritten_by_git() {
    let f = Fixture::new();
    success(&f.setup());
    f.publish("{\"updated\": true}\n");
    success(&f.cli(&["gitlab", "pull"]));
    assert_eq!(
        fs::read_to_string(f.target.join("secrets.json")).unwrap(),
        "{\"updated\": true}\n"
    );
    f.assert_private();
}

#[test]
fn sync_hardens_files_rewritten_by_git() {
    let f = Fixture::new();
    success(&f.setup());
    f.publish("{\"updated\": true}\n");
    success(&f.cli(&["gitlab", "sync"]));
    f.assert_private();
}

#[test]
fn pull_hardens_before_reporting_invalid_json() {
    let f = Fixture::new();
    success(&f.setup());
    f.publish("not json\n");
    let output = f.cli(&["gitlab", "pull"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("拉取后的密钥库损坏"));
    f.assert_private();
}

#[test]
fn failed_rebase_still_hardens_both_files() {
    let f = Fixture::new();
    success(&f.setup());
    for name in ["secrets.json", "meta.json"] {
        fs::write(f.target.join(name), "{\"local\": true}\n").unwrap();
    }
    f.git(&f.target, &["add", "secrets.json", "meta.json"]);
    f.git(&f.target, &["commit", "-m", "conflicting local fixture"]);
    f.publish("{\"remote\": true}\n");
    let output = f.cli(&["gitlab", "pull"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("git pull 出现冲突"));
    assert!(fs::read_to_string(f.target.join("secrets.json"))
        .unwrap()
        .contains("<<<<<<<"));
    f.assert_private();
}

#[test]
fn failed_clone_preserves_error_and_hardens_existing_files() {
    let f = Fixture::new();
    fs::create_dir(&f.target).unwrap();
    for name in ["secrets.json", "meta.json"] {
        fs::write(f.target.join(name), "{}\n").unwrap();
        fs::set_permissions(f.target.join(name), fs::Permissions::from_mode(0o664)).unwrap();
    }
    // Git refuses a nonempty destination; the command must not claim success.
    let output = f.setup();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("克隆失败"));
    f.assert_private();
    assert_eq!(
        fs::read_to_string(f.target.join("secrets.json")).unwrap(),
        "{}\n"
    );
}
