//! Anonymous coarse usage only. Never accept vault data, command arguments or identities.
use rand::RngCore;
use std::process::{Command, Stdio};
use std::time::Duration;

const ENDPOINT: &str = "https://api.webkubor.online/v1/analytics/events";
const TIMEOUT: Duration = Duration::from_millis(800);
pub const HELPER_ARG: &str = "--internal-usage-report";
const OPT_OUT: [&str; 6] = [
    "CI",
    "GITHUB_ACTIONS",
    "DO_NOT_TRACK",
    "DNT",
    "KYVAULT_NO_TELEMETRY",
    "KYVAULT_ANALYTICS_OPTOUT",
];

// ureq's current build does not honor custom proxy/trust settings. Fail closed for
// optional usage reporting instead of connecting directly around those settings.
const NETWORK_ENV: [&str; 16] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "REQUESTS_CA_BUNDLE",
    "CURL_CA_BUNDLE",
    "NODE_EXTRA_CA_CERTS",
    "SSL_CERT",
    "NIX_SSL_CERT_FILE",
    "GRPC_DEFAULT_SSL_ROOTS_FILE_PATH",
];

fn network_settings() -> impl Iterator<Item = (&'static str, std::ffi::OsString)> {
    NETWORK_ENV
        .into_iter()
        .filter_map(|key| std::env::var_os(key).map(|value| (key, value)))
}

fn opted_out(value: &str) -> bool {
    matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

fn enabled_for(
    debug: bool,
    mut values: impl Iterator<Item = String>,
    network_configured: bool,
) -> bool {
    !debug && !network_configured && !values.any(|value| opted_out(&value))
}

pub fn enabled() -> bool {
    enabled_for(
        cfg!(debug_assertions),
        OPT_OUT.iter().filter_map(|key| std::env::var(key).ok()),
        network_settings().any(|(_, value)| !value.is_empty()),
    )
}

fn payload(event: &str) -> Option<serde_json::Value> {
    if !matches!(event, "app.open" | "cli.success") {
        return None;
    }
    let mut id = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut id);
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x3f) | 0x80;
    let hex: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
    let event_id = format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    );
    Some(serde_json::json!({
        "project": "kyvault", "event_id": event_id, "event": event, "properties": {}
    }))
}

fn send(endpoint: &str, body: serde_json::Value) {
    // No credentials, redirects, retries, persisted IDs, output or response-body reads.
    let agent = ureq::AgentBuilder::new()
        .timeout(TIMEOUT)
        .redirects(0)
        .build();
    let _ = agent.post(endpoint).send_json(body);
}

/// Called only after the CLI command has returned success. The parent never waits for HTTP.
pub fn cli_success() {
    if !enabled() {
        return;
    }
    if let Ok(executable) = std::env::current_exe() {
        // The helper receives only network-policy environment, never vault credentials or user arguments.
        let _ = Command::new(executable)
            .arg(HELPER_ARG)
            .env_clear()
            .envs(network_settings())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}

/// Main intercepts this exact private argument before parsing or opening a vault.
pub fn cli_helper() {
    if !enabled() {
        return;
    }
    let (done, received) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new().spawn(move || {
        if let Some(body) = payload("cli.success") {
            send(ENDPOINT, body);
        }
        let _ = done.send(());
    });
    // Bounds the helper process lifetime even if DNS resolution outlasts the HTTP timeout.
    let _ = received.recv_timeout(TIMEOUT);
}

/// Invoke once from successful desktop setup; never on window show/focus or clipboard events.
pub fn app_open() {
    if enabled() {
        let _ = std::thread::Builder::new().spawn(|| {
            if let Some(body) = payload("app.open") {
                send(ENDPOINT, body);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn payload_is_anonymous_allowlisted_and_ephemeral() {
        assert!(payload("secret://fixture/private").is_none());
        for event in ["cli.success", "app.open"] {
            let body = payload(event).unwrap();
            assert_eq!(body.as_object().unwrap().len(), 4);
            assert_eq!(body["project"], "kyvault");
            assert_eq!(body["event"], event);
            assert_eq!(body["properties"], serde_json::json!({}));
            let id = body["event_id"].as_str().unwrap();
            assert_eq!(id.len(), 36);
            assert_eq!(&id[14..15], "4");
            assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
            assert_ne!(body["event_id"], payload(event).unwrap()["event_id"]);
        }
    }

    #[test]
    fn honors_do_not_track_and_explicit_opt_out_values() {
        assert!(!enabled_for(true, std::iter::empty(), false));
        assert!(enabled_for(false, std::iter::empty(), false));
        assert!(!enabled_for(false, std::iter::empty(), true));
        for key in OPT_OUT {
            assert!(!enabled_for(
                false,
                OPT_OUT
                    .iter()
                    .filter(|candidate| **candidate == key)
                    .map(|_| "1".to_owned()),
                false
            ));
        }
        for value in ["1", "true", "TRUE", "yes"] {
            assert!(opted_out(value));
        }
        for value in ["0", "false", "", "no"] {
            assert!(!opted_out(value));
        }
    }

    #[test]
    fn native_post_has_no_credentials_and_failure_is_ignored() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/events", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            connection.set_read_timeout(Some(TIMEOUT)).unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let count = connection.read(&mut chunk).unwrap();
                assert!(count > 0, "client closed before sending a JSON body");
                request.extend_from_slice(&chunk[..count]);
                if request.ends_with(b"}") {
                    break;
                }
            }
            connection.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            String::from_utf8(request).unwrap()
        });
        send(&endpoint, payload("app.open").unwrap());
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /events HTTP/1.1\r\n"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
        assert!(!request.to_ascii_lowercase().contains("cookie:"));
        let (_, body) = request.split_once("\r\n\r\n").unwrap();
        let body: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(body["properties"], serde_json::json!({}));
        assert_eq!(body["event"], "app.open");
    }
}
