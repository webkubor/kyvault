//! The UI invokes this command after confirmation. Repository creation never receives vault data.
use crate::account::{self, CreatedRepo};
use crate::commands::{active, vault, DesktopState};
use serde_json::{json, Value};
use tauri::State;

fn valid_component(s: &str) -> bool {
    let bytes = s.as_bytes();
    (1..=100).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(b))
        && !s.contains("..")
        && !s.to_ascii_lowercase().ends_with(".git")
}

struct Request<'a> {
    provider: &'a str,
    host: &'a str,
    name: &'a str,
    username: &'a str,
}

impl Request<'_> {
    fn validate(&self) -> Result<(), String> {
        if !matches!(self.provider, "github" | "gitlab") {
            return Err("provider".into());
        }
        if self.host.len() > 253
            || !self.host.split('.').all(|label| {
                !label.is_empty()
                    && label.len() <= 63
                    && !label.starts_with('-')
                    && !label.ends_with('-')
                    && label
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            })
        {
            return Err("host".into());
        }
        if !valid_component(self.name) || !valid_component(self.username) {
            return Err("仓库名或用户名无效".into());
        }
        Ok(())
    }

    fn create_remote(
        &self,
        user: u64,
        mut api: impl FnMut(&str, &str, Option<&Value>) -> Result<Value, &'static str>,
    ) -> Result<CreatedRepo, String> {
        let github = self.provider == "github";
        let path = format!("{}/{}", self.username, self.name);
        let location = format!("{}/{}", self.host, path);
        let partial = |stage: &str| {
            format!("已创建 {location}，{stage}。请在平台检查仓库和默认分支；可保留该仓库，另选新名称重新创建。")
        };
        // Both POST endpoints create exclusively in the current user's personal namespace.
        // On conflict or ambiguous response we never fall back to opening an existing project.
        let (endpoint, body) = if github {
            (
                "user/repos",
                json!({"name":self.name,"private":true,"auto_init":false}),
            )
        } else {
            (
                "projects",
                json!({"name":self.name,"path":self.name,"visibility":"private","initialize_with_readme":false}),
            )
        };
        let created = api("POST", endpoint, Some(&body)).map_err(|_| {
            format!("未确认创建成功。请在平台检查 {location}；若已存在，请保留原仓库并另选名称。")
        })?;
        let project = created["id"]
            .as_u64()
            .ok_or_else(|| partial("未取得项目 ID"))?;
        let project_endpoint = if github {
            format!("repos/{path}")
        } else {
            format!("projects/{project}")
        };
        let check = |metadata: &Value| {
            metadata["id"].as_u64() == Some(project)
                && if github {
                    metadata["private"] == true
                        && metadata["visibility"] == "private"
                        && metadata["full_name"] == path
                        && metadata["owner"]["id"].as_u64() == Some(user)
                        && metadata["owner"]["type"] == "User"
                } else {
                    metadata["visibility"] == "private"
                        && metadata["path_with_namespace"] == path
                        && metadata["namespace"]["kind"] == "user"
                        && metadata["namespace"]["full_path"] == self.username
                }
        };
        let info = api("GET", &project_endpoint, None).map_err(|_| partial("尚未核验项目"))?;
        if !check(&info) {
            return Err(partial("私有状态、项目 ID 或个人命名空间未通过核验"));
        }
        // GitLab can report null for an empty project. Its first commit establishes the
        // default branch. Read it back after the commit, including on that first-branch path.
        let branch = match info.get("default_branch") {
            Some(Value::Null) if !github && info["empty_repo"] == true => "main",
            Some(Value::String(s)) if !s.is_empty() => s,
            _ => return Err(partial("未取得默认分支")),
        };
        let message = "Initialize empty kyvault store";
        let (method, endpoint, body) = if github {
            (
                "PUT",
                format!("{project_endpoint}/contents/secrets.json"),
                json!({"message":message,"content":"e30=","branch":branch}),
            )
        } else {
            (
                "POST",
                format!("{project_endpoint}/repository/files/secrets.json"),
                json!({"commit_message":message,"content":"{}","branch":branch}),
            )
        };
        api(method, &endpoint, Some(&body))
            .map_err(|_| partial("尚未确认空 secrets.json 初始化成功"))?;
        let final_info =
            api("GET", &project_endpoint, None).map_err(|_| partial("尚未核验初始化后的项目"))?;
        if !check(&final_info) || final_info["default_branch"] != branch {
            return Err(partial("初始化后的私有状态、项目身份或默认分支未通过核验"));
        }
        Ok(CreatedRepo {
            provider: self.provider.into(),
            host: self.host.into(),
            path,
            branch: branch.into(),
            username: self.username.into(),
            user,
            project,
        })
    }
}

fn write_api(
    provider: &str,
    host: &str,
    method: &str,
    endpoint: &str,
    body: &Value,
) -> Result<Value, &'static str> {
    use std::process::Stdio;
    let mut child = account::cli(provider)?
        .args([
            "api",
            "--hostname",
            host,
            "-X",
            method,
            "--input",
            "-",
            endpoint,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "cli")?;
    let wrote = child
        .stdin
        .take()
        .is_some_and(|input| serde_json::to_writer(input, body).is_ok());
    let output = child.wait_with_output().map_err(|_| "cli")?;
    if !wrote || !output.status.success() {
        return Err("request");
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "response")
}

#[tauri::command]
pub async fn account_create(
    state: State<'_, DesktopState>,
    provider: String,
    host: String,
    name: String,
    username: String,
) -> Result<Value, String> {
    drop(active(&state).map_err(|_| "locked")?);
    let data_root = state.data_root.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let request = Request {
            provider: &provider,
            host: &host,
            name: &name,
            username: &username,
        };
        request.validate()?;
        let store = vault().map_err(|_| "checkout")?;
        create_and_bind(&request, &store, &data_root)
    })
    .await
    .map_err(|_| "工作线程中断。请先在平台检查目标仓库，再决定是否重试。".to_owned())?
}

fn create_and_bind(
    req: &Request<'_>,
    store: &kyvault::store::Store,
    data_root: &std::path::Path,
) -> Result<Value, String> {
    let mut remote_error = None;
    let mut initialized = false;
    let result = account::create_and_bind(store, data_root, || {
        let (id, actual) = account::verify_identity(req.provider, req.host)?;
        if actual != req.username {
            return Err("auth");
        }
        let repo = req
            .create_remote(id, |method, endpoint, body| match body {
                None => account::api(req.provider, req.host, endpoint),
                Some(body) => write_api(req.provider, req.host, method, endpoint, body),
            })
            .map_err(|err| {
                remote_error = Some(err);
                "create"
            })?;
        initialized = true;
        Ok(repo)
    });
    match (result, remote_error) {
        (_, Some(detail)) => Err(detail),
        (Err(code), _) if initialized => Err(format!(
            "已创建 {}/{}/{} 并初始化空 secrets.json，本地绑定失败（{code}）。请检查本地工作台状态；保留远端后可另选新名称重试。",
            req.host, req.username, req.name
        )),
        (other, _) => other.map_err(str::to_owned),
    }
}

#[cfg(test)]
#[path = "repo_create_fixtures.rs"]
mod fixtures;
