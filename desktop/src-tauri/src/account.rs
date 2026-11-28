//! Account authorization belongs to glab. Sync addresses one existing GitLab
//! file by project ID, so no Git index, history, hooks or tags can be uploaded.
use crate::commands::{active, vault, DesktopState};
use base64::{engine::general_purpose::STANDARD, Engine};
use kyvault::{crypto, store::Store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tauri::State;

static BINDING_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Target {
    #[serde(default = "gitlab_provider")]
    provider: String,
    host: String,
    path: String,
    branch: String,
}

#[derive(Serialize, Deserialize)]
struct Binding {
    root: PathBuf,
    target: Target,
    user: u64,
    username: String,
    project: u64,
    baseline: String,
    #[serde(default)]
    created: bool,
}

fn gitlab_provider() -> String {
    "gitlab".into()
}

pub(super) struct CreatedRepo {
    pub(super) provider: String,
    pub(super) host: String,
    pub(super) path: String,
    pub(super) branch: String,
    pub(super) username: String,
    pub(super) user: u64,
    pub(super) project: u64,
}

fn output(cmd: &mut Command, error: &'static str) -> Result<Vec<u8>> {
    let out = cmd
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .output()
        .map_err(|_| error)?;
    out.status.success().then_some(out.stdout).ok_or(error)
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let bytes = output(Command::new("git").current_dir(root).args(args), "checkout")?;
    String::from_utf8(bytes)
        .map(|s| s.trim().to_owned())
        .map_err(|_| "checkout")
}

fn safe_part(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
}

fn parse_origin(url: &str) -> Result<(String, String)> {
    let (host, path) = if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("ssh://git@"))
    {
        rest.split_once('/')
    } else {
        url.strip_prefix("git@").and_then(|s| s.split_once(':'))
    }
    .ok_or("checkout")?;
    let path = path.strip_suffix(".git").unwrap_or(path);
    if !host.split('.').all(safe_part)
        || host.starts_with('-')
        || !path.contains('/')
        || !path.split('/').all(safe_part)
    {
        return Err("checkout");
    }
    Ok((host.to_ascii_lowercase(), path.into()))
}

fn target(root: &Path) -> Result<Target> {
    let top = git(root, &["rev-parse", "--show-toplevel"])?;
    if fs::canonicalize(top).ok().as_deref() != Some(root) {
        return Err("checkout");
    }
    let origin = parse_origin(&git(root, &["remote", "get-url", "--all", "origin"])?)?;
    if parse_origin(&git(
        root,
        &["remote", "get-url", "--push", "--all", "origin"],
    )?)? != origin
    {
        return Err("checkout");
    }
    let branch = git(root, &["symbolic-ref", "--short", "HEAD"])?;
    if !branch.split('/').all(safe_part) {
        return Err("checkout");
    }
    Ok(Target {
        provider: if origin.0 == "github.com" {
            "github".into()
        } else {
            gitlab_provider()
        },
        host: origin.0,
        path: origin.1,
        branch,
    })
}

pub(super) fn cli(provider: &str) -> Result<Command> {
    let name = match provider {
        "github" => "gh",
        "gitlab" => "glab",
        _ => return Err("provider"),
    };
    let program = std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(name))
                .find(|p| p.is_absolute() && p.is_file())
        })
        .or_else(|| {
            ["/opt/homebrew/bin", "/usr/local/bin"]
                .iter()
                .map(|p| Path::new(p).join(name))
                .find(|p| p.is_file())
        })
        .unwrap_or_else(|| PathBuf::from(name));
    let mut cmd = Command::new(program);
    // Desktop identity must come from the host's saved glab account.
    for var in [
        "GITLAB_TOKEN",
        "GITLAB_ACCESS_TOKEN",
        "OAUTH_TOKEN",
        "CI_JOB_TOKEN",
        "GITLAB_HOST",
        "GITLAB_URI",
        "GITLAB_URL",
        "GITLAB_REPO",
        "GLAB_ENABLE_CI_AUTOLOGIN",
        "GLAB_DEBUG_HTTP",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
        "GH_DEBUG",
        "GH_HOST",
        "KEYRING_MASTER_KEY",
    ] {
        cmd.env_remove(var);
    }
    cmd.current_dir(std::env::temp_dir());
    Ok(cmd)
}

pub(super) fn api(provider: &str, host: &str, endpoint: &str) -> Result<Value> {
    let bytes = output(
        cli(provider)?.args(["api", "--hostname", host, endpoint]),
        "gitlab",
    )?;
    serde_json::from_slice(&bytes).map_err(|_| "gitlab")
}

fn put(t: &Target, endpoint: &str, method: &str, body: &Value) -> Result<()> {
    let mut child = cli(&t.provider)?
        .args([
            "api",
            "--hostname",
            &t.host,
            "-X",
            method,
            "--input",
            "-",
            endpoint,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "gitlab")?;
    let wrote = serde_json::to_writer(child.stdin.take().ok_or("gitlab")?, body);
    let status = child.wait().map_err(|_| "gitlab")?;
    if wrote.is_err() || !status.success() {
        return Err("upload");
    }
    Ok(())
}

pub(super) fn verify_identity(provider: &str, host: &str) -> Result<(u64, String)> {
    if !valid_host(host) {
        return Err("host");
    }
    let user = api(provider, host, "user")?;
    Ok((
        user["id"].as_u64().ok_or("auth")?,
        user[if provider == "github" {
            "login"
        } else {
            "username"
        }]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("auth")?
        .into(),
    ))
}

fn project(t: &Target) -> Result<u64> {
    let github = t.provider == "github";
    let endpoint = if github {
        format!("repos/{}", t.path)
    } else {
        format!("projects/{}", t.path.replace('/', "%2F"))
    };
    let p = api(&t.provider, &t.host, &endpoint)?;
    let private = if github {
        p["private"] == true && p["visibility"] == "private" && p["full_name"] == t.path
    } else {
        p["visibility"] == "private" && p["path_with_namespace"] == t.path
    };
    if !private {
        return Err("private");
    }
    p["id"].as_u64().ok_or("private")
}

fn root(store: &Store) -> Result<PathBuf> {
    fs::canonicalize(store.root()).map_err(|_| "checkout")
}

fn read_binding(data: &Path) -> Result<Binding> {
    let bytes = fs::read(data.join("gitlab-account.json")).map_err(|_| "binding")?;
    serde_json::from_slice(&bytes).map_err(|_| "binding")
}

fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut tmp =
        tempfile::NamedTempFile::new_in(path.parent().ok_or("storage")?).map_err(|_| "storage")?;
    tmp.write_all(bytes)
        .and_then(|_| tmp.as_file().sync_all())
        .map_err(|_| "storage")?;
    tmp.persist(path).map_err(|_| "storage")?;
    Ok(())
}

fn save(data: &Path, b: &Binding) -> Result<()> {
    replace(
        &data.join("gitlab-account.json"),
        &serde_json::to_vec(b).map_err(|_| "storage")?,
    )
}

fn summary(t: &Target, username: Option<&str>, bound: bool) -> Value {
    json!({"provider":t.provider,"host":t.host,"path":t.path,"branch":t.branch,"username":username,"bound":bound})
}

fn key(store: &Store) -> Result<[u8; 32]> {
    crypto::derive_file_key(&store.master_key().map_err(|_| "unlock")?).map_err(|_| "unlock")
}

fn credentials(bytes: &[u8], key: &[u8; 32]) -> Result<Value> {
    let data: Value = serde_json::from_slice(bytes).map_err(|_| "cipher")?;
    for (platform, groups) in data.as_object().ok_or("cipher")? {
        let groups = groups.as_object().ok_or("cipher")?;
        if platform.contains('/') && groups.len() == 1 && groups.contains_key("ciphertext") {
            check_cipher(&groups["ciphertext"], key)?;
            continue;
        }
        for (group, fields) in groups {
            if !matches!(platform.as_str(), "_servers" | "_clis")
                && !matches!(group.as_str(), "keys" | "accounts")
            {
                return Err("cipher");
            }
            for value in fields.as_object().ok_or("cipher")?.values() {
                check_cipher(value, key)?;
            }
        }
    }
    Ok(data)
}

fn check_cipher(value: &Value, key: &[u8; 32]) -> Result<()> {
    crypto::decrypt_joined(value.as_str().ok_or("cipher")?, key)
        .map(|_| ())
        .map_err(|_| "cipher")
}

fn digest(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}

fn local(store: &Store, key: &[u8; 32]) -> Result<Value> {
    let file = store.secrets_path();
    if !file.symlink_metadata().map_err(|_| "cipher")?.is_file() {
        return Err("cipher");
    }
    credentials(&fs::read(file).map_err(|_| "cipher")?, key)
}

#[tauri::command]
pub fn account_status(state: State<DesktopState>) -> Result<Value> {
    let _session = active(&state).map_err(|_| "locked")?;
    let saved = read_binding(&state.data_root).ok();
    let root = vault().ok().and_then(|s| root(&s).ok());
    let matched = saved
        .as_ref()
        .filter(|b| root.as_deref() == Some(&b.root) && matches_root(b, &b.root));
    let mut result = if let Some(b) = matched {
        summary(&b.target, Some(&b.username), true)
    } else if let Some(t) = root.and_then(|r| target(&r).ok()) {
        summary(&t, None, false)
    } else {
        json!({"bound":false})
    };
    result["saved"] = saved
        .map(|b| summary(&b.target, Some(&b.username), true))
        .unwrap_or(Value::Null);
    Ok(result)
}

#[tauri::command]
pub async fn account_login(
    state: State<'_, DesktopState>,
    provider: String,
    host: String,
) -> Result<Value> {
    drop(active(&state).map_err(|_| "locked")?);
    tauri::async_runtime::spawn_blocking(move || login(&provider, &host))
        .await
        .map_err(|_| "auth")?
}

fn valid_host(host: &str) -> bool {
    host.len() <= 253
        && host.split('.').all(|s| {
            !s.is_empty()
                && s.len() <= 63
                && !s.starts_with('-')
                && !s.ends_with('-')
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

fn login(provider: &str, host: &str) -> Result<Value> {
    let (_, username) =
        verify_identity(provider, host).map_err(|e| if e == "gitlab" { "auth" } else { e })?;
    Ok(json!({"provider":provider,"host":host,"username":username}))
}

#[tauri::command]
pub async fn account_bind(
    state: State<'_, DesktopState>,
    provider: String,
    host: String,
    path: String,
    branch: String,
    username: String,
) -> Result<Value> {
    drop(active(&state).map_err(|_| "locked")?);
    let dir = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        bind(
            &vault().map_err(|_| "checkout")?,
            &dir,
            Target {
                provider,
                host,
                path,
                branch,
            },
            &username,
        )
    })
    .await
    .map_err(|_| "binding")?
}

fn bind(store: &Store, dir: &Path, expected: Target, username: &str) -> Result<Value> {
    let _binding_lock = BINDING_LOCK.lock().map_err(|_| "storage")?;
    let root = root(store)?;
    let _lock = store.lock().map_err(|_| "storage")?;
    let mut t = target(&root)?;
    t.provider.clone_from(&expected.provider);
    if t != expected {
        return Err("checkout");
    }
    let (id, name) = verify_identity(&t.provider, &t.host)?;
    if name != username {
        return Err("auth");
    }
    let project = project(&t)?;
    let key = key(store)?;
    local(store, &key)?;
    let base = git(
        &root,
        &[
            "show",
            &format!("refs/remotes/origin/{}:secrets.json", t.branch),
        ],
    )?;
    let baseline = digest(&credentials(base.as_bytes(), &key)?);
    let answer = summary(&t, Some(&name), true);
    save(
        dir,
        &Binding {
            root,
            target: t,
            user: id,
            username: name,
            project,
            baseline,
            created: false,
        },
    )?;
    Ok(answer)
}

fn no_origin(root: &Path) -> bool {
    !root.join(".git").exists()
        || git(root, &["remote"]).is_ok_and(|names| !names.lines().any(|n| n == "origin"))
}

fn matches_root(b: &Binding, root: &Path) -> bool {
    if b.created && no_origin(root) {
        return true;
    }
    target(root).is_ok_and(|t| {
        t.host == b.target.host && t.path == b.target.path && t.branch == b.target.branch
    })
}

pub(super) fn create_and_bind(
    store: &Store,
    dir: &Path,
    create: impl FnOnce() -> Result<CreatedRepo>,
) -> Result<Value> {
    let _binding_lock = BINDING_LOCK.lock().map_err(|_| "storage")?;
    let _store_lock = store.lock().map_err(|_| "storage")?;
    let root = root(store)?;
    if dir
        .join("gitlab-account.json")
        .try_exists()
        .map_err(|_| "storage")?
        || !no_origin(&root)
    {
        return Err("binding");
    }
    if store.secrets_path().try_exists().map_err(|_| "storage")? {
        local(store, &key(store)?)?;
    }
    let repo = create()?;
    bind_created(dir, root, repo).map_err(|_| "createdUnbound")
}

fn bind_created(dir: &Path, root: PathBuf, repo: CreatedRepo) -> Result<Value> {
    let t = Target {
        provider: repo.provider,
        host: repo.host,
        path: repo.path,
        branch: repo.branch,
    };
    if !valid_host(&t.host)
        || !t.path.contains('/')
        || !t.path.split('/').all(safe_part)
        || !t.branch.split('/').all(safe_part)
    {
        return Err("private");
    }
    if project(&t)? != repo.project
        || verify_identity(&t.provider, &t.host)? != (repo.user, repo.username.clone())
    {
        return Err("private");
    }
    let result = summary(&t, Some(&repo.username), true);
    let binding = Binding {
        root,
        target: t,
        user: repo.user,
        username: repo.username,
        project: repo.project,
        baseline: digest(&json!({})),
        created: true,
    };
    if !no_origin(&binding.root)
        || serde_json::from_slice::<Value>(&remote_file(&binding, &endpoint(&binding))?.0)
            .map_err(|_| "cipher")?
            != json!({})
    {
        return Err("binding");
    }
    save(dir, &binding)?;
    Ok(result)
}

fn endpoint(b: &Binding) -> String {
    if b.target.provider == "github" {
        format!("repos/{}/contents/secrets.json", b.target.path)
    } else {
        format!("projects/{}/repository/files/secrets.json", b.project)
    }
}

fn remote(b: &Binding, endpoint: &str, key: &[u8; 32]) -> Result<(Value, String)> {
    let (bytes, rev) = remote_file(b, endpoint)?;
    Ok((credentials(&bytes, key)?, rev))
}

fn remote_file(b: &Binding, endpoint: &str) -> Result<(Vec<u8>, String)> {
    let t = &b.target;
    let file = api(
        &t.provider,
        &t.host,
        &format!("{endpoint}?ref={}", t.branch.replace('/', "%2F")),
    )?;
    let github = t.provider == "github";
    if file[if github { "path" } else { "file_path" }] != "secrets.json"
        || file["encoding"] != "base64"
        || (github && file["type"] != "file")
    {
        return Err("cipher");
    }
    let bytes = STANDARD
        .decode(
            file["content"]
                .as_str()
                .ok_or("cipher")?
                .replace(['\n', '\r'], ""),
        )
        .map_err(|_| "cipher")?;
    let rev = file[if github { "sha" } else { "last_commit_id" }]
        .as_str()
        .filter(|s| matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("gitlab")?;
    Ok((bytes, rev.to_owned()))
}

fn sync(store: &Store, dir: &Path, expected: &Target, username: &str) -> Result<&'static str> {
    let _binding_lock = BINDING_LOCK.lock().map_err(|_| "storage")?;
    let _lock = store.lock().map_err(|_| "storage")?;
    let mut b = read_binding(dir)?;
    if &b.target != expected || username != b.username {
        return Err("binding");
    }
    let root = root(store)?;
    if b.root != root || !matches_root(&b, &root) {
        return Err("checkout");
    }
    if verify_identity(&b.target.provider, &b.target.host)?.0 != b.user
        || project(&b.target)? != b.project
    {
        return Err("binding");
    }
    let key = key(store)?;
    let current = local(store, &key)?;
    let github = b.target.provider == "github";
    let endpoint = endpoint(&b);
    let here = digest(&current);
    let (upstream, rev) = remote(&b, &endpoint, &key)?;
    let there = digest(&upstream);
    let outcome = if here == there {
        "same"
    } else if there == b.baseline {
        let content = current.to_string();
        let body = if github {
            json!({"branch":b.target.branch,"message":"Sync encrypted kyvault records","content":STANDARD.encode(content),"sha":rev})
        } else {
            json!({"branch":b.target.branch,"commit_message":"Sync encrypted kyvault records","content":content,"last_commit_id":rev})
        };
        put(&b.target, &endpoint, "PUT", &body)?;
        if digest(&remote(&b, &endpoint, &key)?.0) != here {
            return Err("verify");
        }
        "uploaded"
    } else if here == b.baseline {
        // Check the incoming snapshot against local integrity metadata before
        // replacing anything. A mismatch requires reconciling the two files.
        if store.meta_file().exists() {
            for (reference, entry) in store.load_meta().map_err(|_| "metadata")? {
                let Some(want) = entry["sha256"].as_str() else {
                    continue;
                };
                let Some((platform, name)) = reference.split_once('/') else {
                    continue;
                };
                let group = &upstream[platform];
                let field = group["keys"]
                    .get(name)
                    .or_else(|| group["accounts"].get(name))
                    .or_else(|| upstream[&reference].get("ciphertext"));
                if let Some(cipher) = field.and_then(Value::as_str) {
                    let value = crypto::decrypt_joined(cipher, &key).map_err(|_| "cipher")?;
                    if kyvault::model::sha256_hex(&value) != want {
                        return Err("metadata");
                    }
                }
            }
        }
        replace(&store.secrets_path(), upstream.to_string().as_bytes())?;
        "downloaded"
    } else {
        return Err("conflict");
    };
    b.baseline = if outcome == "downloaded" { there } else { here };
    save(dir, &b)?;
    Ok(outcome)
}

#[tauri::command]
pub async fn account_sync(
    state: State<'_, DesktopState>,
    provider: String,
    host: String,
    path: String,
    branch: String,
    username: String,
) -> Result<&'static str> {
    drop(active(&state).map_err(|_| "locked")?);
    let dir = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        sync(
            &vault().map_err(|_| "checkout")?,
            &dir,
            &Target {
                provider,
                host,
                path,
                branch,
            },
            &username,
        )
    })
    .await
    .map_err(|_| "gitlab")?
}

fn unbind(dir: &Path, expected: Target, username: &str) -> Result<()> {
    let _guard = BINDING_LOCK.lock().map_err(|_| "storage")?;
    let b = read_binding(dir)?;
    if b.target != expected || username != b.username {
        return Err("binding");
    }
    fs::remove_file(dir.join("gitlab-account.json")).map_err(|_| "storage")
}

#[tauri::command]
pub async fn account_unbind(
    state: State<'_, DesktopState>,
    provider: String,
    host: String,
    path: String,
    branch: String,
    username: String,
) -> Result<()> {
    drop(active(&state).map_err(|_| "locked")?);
    let dir = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        unbind(
            &dir,
            Target {
                provider,
                host,
                path,
                branch,
            },
            &username,
        )
    })
    .await
    .map_err(|_| "storage")?
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn rejects_ambiguous_remotes() {
        for url in [
            "https://token@host/g/v",
            "file:///tmp/v",
            "https://host/g/../v",
            "https://host/g/v\nhttps://host/g/x",
            "http://host/g/v",
        ] {
            assert!(parse_origin(url).is_err());
        }
        assert_eq!(
            parse_origin("git@example.test:group/sub/v.git"),
            parse_origin("https://example.test/group/sub/v")
        );
    }

    fn executable(path: PathBuf, source: &str) {
        fs::write(&path, source).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn isolated_account_lifecycle() {
        // A child process gives fake commands a private PATH without changing
        // environment variables in parallel desktop tests.
        if let Some(dir) = std::env::var_os("KY_ACCOUNT_FIXTURE_CHILD") {
            fixture(Path::new(&dir));
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        executable(
            dir.join("git"),
            r#"#!/bin/sh
case "$*" in
  'rev-parse --show-toplevel') pwd -P ;;
  'remote get-url --all origin'|'remote get-url --push --all origin') cat "$HOME/origin" ;;
  'symbolic-ref --short HEAD') echo main ;;
  'show refs/remotes/origin/main:secrets.json') cat "$HOME/base" ;;
  *) exit 81 ;;
esac
"#,
        );
        executable(
            dir.join("glab"),
            r#"#!/usr/bin/python3
import sys, os, json, base64, pathlib
d = pathlib.Path(os.environ['HOME'])
a = sys.argv[1:]
assert a[:3] == ['api','--hostname','example.test'] and (d/'session').exists()
ep = 'projects/11/repository/files/secrets.json'
if a[3:] == ['user']:
    print(json.dumps(dict(id=7,username='fixture')))
elif a[3:] == ['projects/group%2Fvault']:
    print(json.dumps(dict(id=11,visibility=(d/'visibility').read_text(),path_with_namespace='group/vault')))
elif a[3:] == [ep+'?ref=main']:
    print(json.dumps(dict(file_path='secrets.json',encoding='base64',content=base64.b64encode((d/'remote').read_bytes()).decode(),last_commit_id='a'*40)))
elif a[3:] == ['-X','PUT','--input','-',ep]:
    body = json.load(sys.stdin)
    assert body['branch']=='main' and body['last_commit_id']=='a'*40
    assert set(body)=={'branch','commit_message','content','last_commit_id'}
    assert 'fixture-password' not in body['content']
    (d/'remote').write_text(body['content'])
else:
    sys.exit(80)
"#,
        );
        executable(
            dir.join("gh"),
            r#"#!/usr/bin/python3
import os, sys, json, base64, pathlib
d=pathlib.Path(os.environ["HOME"])
a=sys.argv[1:]
assert a[:3]==["api","--hostname","hub.test"]
ep="repos/fixture/new/contents/secrets.json"
if a[3:]==["user"]:
    print(json.dumps(dict(id=42,login="fixture")))
elif a[3:]==["repos/fixture/new"]:
    print(json.dumps(dict(id=99,private=True,visibility="private",full_name="fixture/new")))
elif a[3:]==[ep+"?ref=main"]:
    print(json.dumps(dict(type="file",path="secrets.json",encoding="base64",content=base64.encodebytes((d/"hub-file").read_bytes()).decode(),sha="a"*40)))
elif a[3:]==["-X","PUT","--input","-",ep]:
    body=json.load(sys.stdin)
    assert body["sha"]=="a"*40 and body["branch"]=="main"
    assert set(body)=={"message","sha","branch","content"}
    if (d/"hub-reject").exists(): sys.exit(1)
    (d/"hub-file").write_bytes(base64.b64decode(body["content"]))
else: sys.exit(81)
"#,
        );
        let result = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "account::tests::isolated_account_lifecycle",
                "--nocapture",
            ])
            .env_clear()
            .env("HOME", dir)
            .env(
                "PATH",
                std::env::join_paths([dir, Path::new("/usr/bin"), Path::new("/bin")]).unwrap(),
            )
            .env("KY_ACCOUNT_FIXTURE_CHILD", dir)
            .env("KYVAULT_STORE_DIR", dir.join("checkout"))
            .env("KY_DESKTOP_DATA_DIR", dir.join("desktop"))
            .status()
            .unwrap();
        assert!(result.success());
    }

    fn fixture(dir: &Path) {
        let store = Store::new(dir.join("checkout"));
        let data = dir.join("desktop");
        fs::create_dir(&data).unwrap();
        // No local checkout or key exists during either login.
        assert_eq!(login("gitlab", "example.test"), Err("auth"));
        fs::write(dir.join("session"), "").unwrap();
        assert_eq!(
            login("gitlab", "example.test").unwrap()["username"],
            "fixture"
        );
        assert_eq!(
            login("gitlab", "example.test").unwrap()["username"],
            "fixture"
        );
        assert!(login("gitlab", "--hostname").is_err());
        fs::create_dir(store.root()).unwrap();
        let root = root(&store).unwrap();
        fs::write(dir.join("origin"), "git@example.test:group/vault.git").unwrap();
        fs::write(dir.join("visibility"), "private").unwrap();
        fs::write(dir.join("base"), "{}").unwrap();
        fs::write(dir.join("remote"), "{}").unwrap();
        fs::write(store.secrets_path(), "{}").unwrap();
        let target = || super::target(&root).unwrap();
        assert_eq!(bind(&store, &data, target(), "fixture"), Err("unlock"));
        store.init_master_key().unwrap();
        fs::write(dir.join("visibility"), "public").unwrap();
        assert_eq!(bind(&store, &data, target(), "fixture"), Err("private"));
        fs::write(dir.join("visibility"), "private").unwrap();
        assert!(bind(&store, &data, target(), "fixture").is_ok());
        store
            .set_secret("secret://example/name", "fixture-password")
            .unwrap();
        let run = || sync(&store, &data, &target(), "fixture");
        assert_eq!(run(), Ok("uploaded"));
        assert_eq!(run(), Ok("same"));
        let uploaded = fs::read(dir.join("remote")).unwrap();
        store
            .set_secret("secret://example/name", "fixture-change")
            .unwrap();
        fs::write(dir.join("remote"), "{}").unwrap();
        let edited = fs::read(store.secrets_path()).unwrap();
        assert_eq!(run(), Err("conflict"));
        assert_eq!(fs::read(store.secrets_path()).unwrap(), edited);
        fs::write(
            store.secrets_path(),
            br#"{"example":{"keys":{"name":"plaintext"}}}"#,
        )
        .unwrap();
        assert_eq!(run(), Err("cipher"));
        fs::write(store.secrets_path(), uploaded).unwrap();
        assert_eq!(run(), Ok("downloaded"));
        fs::write(dir.join("origin"), "https://different.test/group/vault.git").unwrap();
        assert_eq!(run(), Err("binding"));
        fs::write(dir.join("origin"), "git@example.test:group/vault.git").unwrap();
        unbind(&data, target(), "fixture").unwrap();
        assert_eq!(run(), Err("binding"));
        assert!(dir.join("session").exists());
        // Non-checkout explicit binding has an empty remote baseline. Only
        // the later sync is allowed to transmit local ciphertext.
        fs::write(dir.join("hub-file"), "{}").unwrap();
        store.set_secret("secret://local/demo", "fixture").unwrap();
        let make = || {
            Ok(CreatedRepo {
                provider: "github".into(),
                host: "hub.test".into(),
                path: "fixture/new".into(),
                branch: "main".into(),
                username: "fixture".into(),
                user: 42,
                project: 99,
            })
        };
        let answer = create_and_bind(&store, &data, make).unwrap();
        assert_eq!(answer["provider"], "github");
        assert_eq!(fs::read_to_string(dir.join("hub-file")).unwrap(), "{}");
        assert!(!store.root().join(".git").exists());
        assert_eq!(
            create_and_bind(&store, &data, || panic!("already bound")),
            Err("binding")
        );
        let bound = read_binding(&data).unwrap();
        let hub_sync = || sync(&store, &data, &bound.target, "fixture");
        fs::write(dir.join("hub-reject"), "").unwrap();
        assert_eq!(hub_sync(), Err("upload"));
        assert_eq!(fs::read_to_string(dir.join("hub-file")).unwrap(), "{}");
        fs::remove_file(dir.join("hub-reject")).unwrap();
        assert_eq!(hub_sync(), Ok("uploaded"));
        assert_eq!(hub_sync(), Ok("same"));
        store.set_secret("secret://local/demo", "changed").unwrap();
        fs::write(dir.join("hub-file"), "{}").unwrap();
        assert_eq!(hub_sync(), Err("conflict"));
        fs::create_dir(store.root().join(".git")).unwrap();
        assert_eq!(hub_sync(), Err("checkout"));
    }
}

#[cfg(test)]
pub(super) fn live_checkout_bind(store: &Store, dir: &Path, description: &Value) -> Result<()> {
    let expected = serde_json::from_value(description.clone()).map_err(|_| "target")?;
    let username = description["username"].as_str().ok_or("username")?;
    bind(store, dir, expected, username).map(|_| ())
}

#[cfg(test)]
pub(super) fn live_sync(store: &Store, dir: &Path, description: &Value) -> Result<&'static str> {
    let expected = serde_json::from_value(description.clone()).map_err(|_| "target")?;
    let username = description["username"].as_str().ok_or("username")?;
    sync(store, dir, &expected, username)
}
