use super::*;

fn request(provider: &str) -> Request<'_> {
    Request {
        provider,
        host: "fixture.invalid",
        name: "my-vault",
        username: "tester",
    }
}

fn metadata(provider: &str) -> Value {
    if provider == "github" {
        json!({"id":29,"private":true,"visibility":"private","full_name":"tester/my-vault","default_branch":"team/default",
               "owner":{"id":7,"type":"User"}})
    } else {
        json!({"id":29,"visibility":"private","path_with_namespace":"tester/my-vault","default_branch":"team/default",
               "namespace":{"kind":"user","full_path":"tester"}})
    }
}

#[test]
fn private_projects_initialize_only_a_literal_empty_object() {
    for provider in ["github", "gitlab"] {
        let github = provider == "github";
        let req = request(provider);
        let info = metadata(provider);
        let mut calls = Vec::new();
        let repo = req
            .create_remote(7, |m, e, b| {
                calls.push((m.to_owned(), e.to_owned(), b.cloned()));
                Ok(info.clone())
            })
            .unwrap();
        assert_eq!(
            (repo.project, repo.path.as_str(), repo.branch.as_str()),
            (29, "tester/my-vault", "team/default")
        );
        assert_eq!(calls.len(), 4);
        let project = if github {
            "repos/tester/my-vault"
        } else {
            "projects/29"
        };
        assert_eq!(calls[0].0, "POST");
        assert_eq!(calls[0].1, if github { "user/repos" } else { "projects" });
        assert_eq!(
            calls[0].2,
            Some(if github {
                json!({"name":"my-vault","private":true,"auto_init":false})
            } else {
                json!({"name":"my-vault","path":"my-vault","visibility":"private","initialize_with_readme":false})
            })
        );
        assert_eq!(calls[1], ("GET".into(), project.into(), None));
        assert_eq!(calls[2].0, if github { "PUT" } else { "POST" });
        assert_eq!(
            calls[2].1,
            format!(
                "{project}/{}",
                if github {
                    "contents/secrets.json"
                } else {
                    "repository/files/secrets.json"
                }
            )
        );
        assert_eq!(
            calls[2].2,
            Some(if github {
                json!({"branch":"team/default","message":"Initialize empty kyvault store","content":"e30="})
            } else {
                json!({"branch":"team/default","commit_message":"Initialize empty kyvault store","content":"{}"})
            })
        );
        assert_eq!(calls[3], calls[1]);
    }
}

#[test]
fn invalid_names_and_hosts_are_rejected_before_contact() {
    for name in [
        "",
        "../vault",
        "owner/vault",
        "-flag",
        "repo.git",
        "My Vault",
        "repo?name",
        "a%2Fb",
    ] {
        assert!(Request {
            name,
            ..request("github")
        }
        .validate()
        .is_err());
    }
    for host in ["", "--help", "host/url", "user@host"] {
        assert!(Request {
            host,
            ..request("github")
        }
        .validate()
        .is_err());
    }
    assert!(request("git").validate().is_err());
    assert!(request("gitlab").validate().is_ok());
}

#[test]
fn existing_projects_and_uncertain_creation_never_open_an_existing_repo() {
    for provider in ["github", "gitlab"] {
        let mut calls = 0;
        let err = request(provider)
            .create_remote(7, |method, _, _| {
                calls += 1;
                assert_eq!(method, "POST");
                Err("conflict")
            })
            .err()
            .unwrap();
        assert_eq!(calls, 1);
        assert!(err.contains("fixture.invalid/tester/my-vault"));
    }
}

#[test]
fn privacy_identity_and_project_id_must_match_before_initialization() {
    for (provider, field, wrong) in [
        ("github", "private", json!(false)),
        ("github", "owner", json!({"type":"User","id":8})),
        ("gitlab", "visibility", json!("internal")),
        ("gitlab", "id", json!(30)),
        (
            "gitlab",
            "namespace",
            json!({"kind":"group","full_path":"tester"}),
        ),
    ] {
        let mut info = metadata(provider);
        info[field] = wrong;
        let mut calls = 0;
        assert!(request(provider)
            .create_remote(7, |method, _, _| {
                calls += 1;
                if calls == 1 {
                    Ok(json!({"id":29}))
                } else {
                    assert_eq!(method, "GET");
                    Ok(info.clone())
                }
            })
            .is_err());
        assert_eq!(calls, 2);
    }
}

#[test]
fn gitlab_blank_project_checks_the_first_branch_became_the_default() {
    for final_branch in ["main", "other"] {
        let mut calls = 0;
        let result = request("gitlab").create_remote(7, |_, _, body| {
            calls += 1;
            let mut info = metadata("gitlab");
            match calls {
                2 => {
                    info["default_branch"] = Value::Null;
                    info["empty_repo"] = json!(true);
                }
                3 => assert_eq!(body.unwrap()["branch"], "main"),
                4 => info["default_branch"] = json!(final_branch),
                _ => {}
            }
            Ok(info)
        });
        assert_eq!(result.is_ok(), final_branch == "main");
    }
}

#[cfg(unix)]
#[test]
fn non_git_vault_binds_only_after_mock_cli_verifies_empty_remote() {
    use kyvault::store::Store;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};
    const CHILD: &str = "KY_REPO_CREATE_TEST_CHILD";
    if let Some(root) = std::env::var_os(CHILD) {
        let root = PathBuf::from(root);
        let store = Store::new(root.join("vault"));
        let data = root.join("data");
        fs::create_dir_all(store.root()).unwrap();
        fs::create_dir(&data).unwrap();
        store.init_master_key().unwrap();
        store
            .set_secret("secret://fixture/example", "fixture-only")
            .unwrap();
        let before = fs::read(store.secrets_path()).unwrap();
        let req = request("github");
        assert_eq!(
            create_and_bind(
                &Request {
                    username: "another",
                    ..req
                },
                &store,
                &data
            ),
            Err("auth".into())
        );
        assert!(!root.join("created").exists());
        assert_eq!(create_and_bind(&req, &store, &data).unwrap()["bound"], true);
        let saved: Value =
            serde_json::from_slice(&fs::read(data.join("gitlab-account.json")).unwrap()).unwrap();
        assert_eq!(
            saved["baseline"],
            "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
        assert_eq!(saved["project"], 29);
        assert_eq!(fs::read(store.secrets_path()).unwrap(), before);
        assert_eq!(create_and_bind(&req, &store, &data), Err("binding".into()));
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let gh = tmp.path().join("gh");
    fs::write(&gh, r#"#!/bin/sh
set -eu
[ "$1 $2 $3" = "api --hostname fixture.invalid" ]
shift 3
cd "$KY_REPO_CREATE_TEST_CHILD"
case "$*" in
 user) echo '{"id":7,"login":"tester"}';;
 '-X POST --input - user/repos')
    test ! -e created
    test "$(cat)" = '{"auto_init":false,"name":"my-vault","private":true}'
    touch created
    echo '{"id":29}';;
 repos/tester/my-vault)
    test -f created
    echo '{"id":29,"private":true,"visibility":"private","full_name":"tester/my-vault","owner":{"type":"User","id":7},"default_branch":"trunk"}';;
 '-X PUT --input - repos/tester/my-vault/contents/secrets.json')
    test "$(cat)" = '{"branch":"trunk","content":"e30=","message":"Initialize empty kyvault store"}'
    touch empty-file
    echo '{}';;
 'repos/tester/my-vault/contents/secrets.json?ref=trunk')
    test -f empty-file
    echo '{"path":"secrets.json","type":"file","encoding":"base64","content":"e30=","sha":"0000000000000000000000000000000000000000"}';;
 *) exit 1;;
esac
"#).unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o700)).unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "repo_create::fixtures::non_git_vault_binds_only_after_mock_cli_verifies_empty_remote",
            "--nocapture",
        ])
        .env(CHILD, tmp.path())
        .env("HOME", tmp.path())
        .env("KYVAULT_STORE_DIR", tmp.path().join("vault"))
        .env("KY_DESKTOP_DATA_DIR", tmp.path().join("data"))
        .env("PATH", format!("{}:/usr/bin:/bin", tmp.path().display()))
        .status()
        .unwrap();
    assert!(status.success());
    assert!(tmp.path().join("empty-file").is_file());
}

#[test]
fn failure_after_creation_reports_remote_location_and_stops_writes() {
    for failure_at in [2, 3, 4] {
        let mut calls = 0;
        let failure = request("github")
            .create_remote(7, |_, _, _| {
                calls += 1;
                if calls == failure_at {
                    Err("mock failure")
                } else {
                    Ok(metadata("github"))
                }
            })
            .err()
            .unwrap();
        assert_eq!(calls, failure_at);
        assert!(failure.contains("已创建 fixture.invalid/tester/my-vault"));
        assert!(failure.contains("另选新名称"));
    }
}
