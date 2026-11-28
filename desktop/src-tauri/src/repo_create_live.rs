//! Explicit live acceptance. Each provider creates one personal private repo.
//! Set KY_LIVE_ROOT to a fresh tempdir, KY_LIVE_NAME to a unique kyvault-qa-
//! name and KY_LIVE_HOST to the authenticated host. Run one ignored test.
use super::*;
use anyhow::{anyhow, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use kyvault::store::Store;
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

fn git(provider: &str, root: &Path, args: &[&str]) -> Result<String> {
    let helper = match provider {
        "github" => "!gh auth git-credential",
        "gitlab" => "!glab auth git-credential",
        _ => anyhow::bail!("unknown provider"),
    };
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .args([
            "-c",
            "credential.helper=",
            "-c",
            &format!("credential.helper={helper}"),
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    ensure!(output.status.success(), "fixture Git {} failed", args[0]);
    Ok(String::from_utf8(output.stdout)?.trim().into())
}

fn acceptance(provider: &str) -> Result<()> {
    const REF: &str = "secret://acceptance/roundtrip";
    const FIXTURE: &str = "synthetic comparison value for repository acceptance";
    let temp = std::env::temp_dir().canonicalize()?;
    let base = std::path::PathBuf::from(std::env::var("KY_LIVE_ROOT")?).canonicalize()?;
    ensure!(
        base.parent() == Some(&temp)
            && base
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with("ky-live-")),
        "use a fresh ky-live-* tempdir"
    );
    ensure!(
        std::env::var_os("KEYRING_MASTER_KEY").is_none(),
        "run without a master-key override"
    );
    let dir = base.join(provider);
    fs::create_dir(&dir).context("use a fresh fixture directory")?;
    let host = std::env::var("KY_LIVE_HOST")?;
    let name = std::env::var("KY_LIVE_NAME")?;
    ensure!(
        name.starts_with("kyvault-qa-"),
        "test repository name required"
    );
    let (user, username) =
        account::verify_identity(provider, &host).map_err(|e| anyhow!("identity: {e}"))?;
    let req = Request {
        provider,
        host: &host,
        name: &name,
        username: &username,
    };
    req.validate().map_err(anyhow::Error::msg)?;
    let store = Store::new(dir.join("sender"));
    let sender = dir.join("sender-state");
    let receiver = dir.join("receiver-state");
    fs::create_dir(&sender)?;
    fs::create_dir(&receiver)?;
    fs::create_dir(store.root())?;
    store.init_master_key()?;
    store.set_secret(REF, FIXTURE)?;
    let desc = create_and_bind(&req, &store, &sender).map_err(anyhow::Error::msg)?;
    let path = desc["path"].as_str().context("missing project")?;
    let branch = desc["branch"].as_str().context("missing branch")?;
    ensure!(
        path == format!("{username}/{name}"),
        "not personal namespace"
    );
    let url = format!("https://{host}/{path}");
    println!("CREATED {url}");
    // Clone just the empty remote first, so the receiver has its own checkout
    // and obtains its real base revision independently from the sender.
    let checkout = dir.join("receiver");
    git(
        provider,
        &dir,
        &[
            "clone",
            "--quiet",
            "--branch",
            branch,
            &format!("{url}.git"),
            "receiver",
        ],
    )?;
    let incoming = Store::new(&checkout);
    ensure!(
        incoming.load()? == json!({}),
        "remote was not initialized empty"
    );
    fs::copy(store.master_key_path(), incoming.master_key_path())?;
    account::live_checkout_bind(&incoming, &receiver, &desc).map_err(anyhow::Error::msg)?;
    ensure!(
        account::live_sync(&store, &sender, &desc).map_err(anyhow::Error::msg)? == "uploaded",
        "sender did not upload"
    );
    let read = |ep: &str| account::api(provider, &host, ep).map_err(anyhow::Error::msg);
    let (sha, blob) = if provider == "github" {
        let repo = format!("repos/{path}");
        let metadata = read(&repo)?;
        ensure!(
            metadata["private"] == true
                && metadata["visibility"] == "private"
                && metadata["owner"]["id"] == user
                && metadata["owner"]["type"] == "User",
            "GitHub owner/visibility changed"
        );
        let sha = read(&format!("{repo}/commits/{branch}"))?["sha"]
            .as_str()
            .context("missing commit")?
            .to_owned();
        let tree = read(&format!("{repo}/git/trees/{sha}?recursive=1"))?;
        let entries = tree["tree"].as_array().context("missing tree")?;
        ensure!(
            tree["truncated"] == false
                && entries.len() == 1
                && entries[0]["path"] == "secrets.json"
                && entries[0]["type"] == "blob",
            "unexpected remote tree"
        );
        let id = entries[0]["sha"].as_str().context("missing blob")?;
        (sha, read(&format!("{repo}/git/blobs/{id}"))?)
    } else {
        let repo = format!("projects/{}", path.replace('/', "%2F"));
        let metadata = read(&repo)?;
        ensure!(
            metadata["visibility"] == "private"
                && metadata["namespace"]["kind"] == "user"
                && metadata["namespace"]["full_path"] == username,
            "GitLab owner/visibility changed"
        );
        let sha = read(&format!("{repo}/repository/branches/{branch}"))?["commit"]["id"]
            .as_str()
            .context("missing commit")?
            .to_owned();
        let tree = read(&format!(
            "{repo}/repository/tree?ref={sha}&recursive=true&per_page=100"
        ))?;
        let entries = tree.as_array().context("missing tree")?;
        ensure!(
            entries.len() == 1
                && entries[0]["path"] == "secrets.json"
                && entries[0]["type"] == "blob",
            "unexpected remote tree"
        );
        let id = entries[0]["id"].as_str().context("missing blob")?;
        (sha, read(&format!("{repo}/repository/blobs/{id}"))?)
    };
    let bytes = STANDARD.decode(
        blob["content"]
            .as_str()
            .context("missing contents")?
            .replace(['\r', '\n'], ""),
    )?;
    ensure!(
        !bytes
            .windows(FIXTURE.len())
            .any(|w| w == FIXTURE.as_bytes()),
        "plaintext in remote blob"
    );
    let records: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        records == store.load()?,
        "pinned remote blob differs from encrypted sender"
    );
    ensure!(
        account::live_sync(&incoming, &receiver, &desc).map_err(anyhow::Error::msg)?
            == "downloaded",
        "receiver did not pull"
    );
    ensure!(
        incoming.get_secret(REF)?.as_deref() == Some(FIXTURE),
        "pulled fixture mismatch"
    );
    // Independent Git transport checks the same branch head and exact bytes.
    git(provider, &checkout, &["fetch", "--quiet", "origin", branch])?;
    ensure!(
        git(provider, &checkout, &["rev-parse", "FETCH_HEAD"])? == sha,
        "remote head changed"
    );
    ensure!(
        git(provider, &checkout, &["show", "FETCH_HEAD:secrets.json"])?.as_bytes() == bytes,
        "Git/API blob mismatch"
    );
    println!("PASS provider={provider} url={url} branch={branch} commit={sha} private_personal=true files=1 encrypted=true decrypted_match=true");
    fs::remove_file(store.master_key_path())?;
    fs::remove_file(incoming.master_key_path())?;
    Ok(())
}

#[test]
#[ignore = "creates a real personal private GitHub repository"]
fn github() -> Result<()> {
    acceptance("github")
}

#[test]
#[ignore = "creates a real personal private GitLab project"]
fn gitlab() -> Result<()> {
    acceptance("gitlab")
}
