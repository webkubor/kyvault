//! Run with `cargo test --release --test analytics_cli` to exercise production gates.
//! Every helper invocation has an explicit opt-out; these tests never send live events.
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn helper_respects_opt_out_and_network_policy_without_output() {
    for (key, value) in [
        ("DO_NOT_TRACK", "1"),
        ("DNT", "true"),
        ("KYVAULT_NO_TELEMETRY", "yes"),
        ("KYVAULT_ANALYTICS_OPTOUT", "1"),
        ("CI", "true"),
        ("GITHUB_ACTIONS", "true"),
        ("HTTPS_PROXY", "http://127.0.0.1:1"),
        ("http_proxy", "http://127.0.0.1:1"),
        ("SSL_CERT_FILE", "/nonexistent-fixture-ca.pem"),
    ] {
        let start = Instant::now();
        let result = Command::new(env!("CARGO_BIN_EXE_kyvault"))
            .arg("--internal-usage-report")
            .env_clear()
            .env(key, value)
            .output()
            .unwrap();
        assert!(result.status.success(), "{key}");
        assert!(result.stdout.is_empty(), "{key}");
        assert!(result.stderr.is_empty(), "{key}");
        assert!(start.elapsed() < Duration::from_millis(750), "{key}");
    }
}

#[test]
fn helper_stays_private_and_rejects_additional_arguments() {
    let help = Command::new(env!("CARGO_BIN_EXE_kyvault"))
        .arg("--help")
        .env("KYVAULT_NO_TELEMETRY", "1")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(!String::from_utf8_lossy(&help.stdout).contains("internal-usage"));
    let invalid = Command::new(env!("CARGO_BIN_EXE_kyvault"))
        .args(["--internal-usage-report", "extra-fixture-argument"])
        .env("KYVAULT_NO_TELEMETRY", "1")
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
}
