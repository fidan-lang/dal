//! No live AWS access: exercise the actual default provider chain and client identity cache.
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime},
};

fn write_credentials(root: &Path, identity: &str) -> SystemTime {
    let expiry = SystemTime::now() + Duration::from_secs(2);
    fs::write(
        root.join("credentials.json"),
        serde_json::json!({
            "Version": 1, "AccessKeyId": identity,
            "SecretAccessKey": "synthetic-test-secret", "SessionToken": "synthetic-test-session",
            "Expiration": chrono::DateTime::<chrono::Utc>::from(expiry).to_rfc3339(),
        })
        .to_string(),
    )
    .unwrap();
    expiry
}

async fn exercise_profile(root: &Path, profile: &str) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut identities = Vec::new();
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let read = stream.read(&mut chunk).unwrap();
                assert!(read > 0);
                bytes.extend_from_slice(&chunk[..read]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    let total = end + 4 + length;
                    while bytes.len() < total {
                        let read = stream.read(&mut chunk).unwrap();
                        assert!(read > 0);
                        bytes.extend_from_slice(&chunk[..read]);
                    }
                    break;
                }
            }
            identities.push(String::from_utf8(bytes).unwrap());
            let body = r#"{"Attributes":{"QueueArn":"arn:aws:sqs:eu-central-1:000000000000:fixture.fifo"}}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/x-amz-json-1.0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
        identities
    });
    let identity = format!("ASIAFIXTURE{profile}FIRST");
    let expiry = write_credentials(root, &identity);
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("eu-central-1"))
        .retry_config(aws_config::retry::RetryConfig::disabled())
        .timeout_config(
            aws_config::timeout::TimeoutConfig::builder()
                .operation_timeout(Duration::from_secs(10))
                .build(),
        )
        .load()
        .await;
    let sqs = aws_sdk_sqs::Client::from_conf(
        aws_sdk_sqs::config::Builder::from(&config)
            .endpoint_url(format!("http://{address}"))
            .build(),
    );
    let request = || {
        sqs.get_queue_attributes()
            .queue_url(format!("http://{address}/fixture"))
            .send()
    };
    request().await.unwrap();
    tokio::time::sleep(
        expiry.duration_since(SystemTime::now()).unwrap_or_default() + Duration::from_millis(50),
    )
    .await;
    let renewed = format!("ASIAFIXTURE{profile}RENEWED");
    let expiry = write_credentials(root, &renewed);
    request().await.unwrap();
    let requests = server.join().unwrap();
    assert!(requests[0].contains(&identity));
    assert!(
        requests[1].contains(&renewed),
        "Client must renew expired process credentials"
    );
    tokio::time::sleep(
        expiry.duration_since(SystemTime::now()).unwrap_or_default() + Duration::from_millis(50),
    )
    .await;
    fs::write(root.join("fail"), "fail").unwrap();
    let failure = request().await.unwrap_err();
    let message = format!("{failure:?}").to_lowercase();
    assert!(
        message.contains("credential") || message.contains("identity"),
        "Failure must originate in credential loading, not service/network fallback"
    );
}

#[test]
fn profile_credentials_refresh_and_fail_closed() {
    if let Ok(root) = std::env::var("DAL_CREDENTIAL_TEST_CHILD") {
        let profile = std::env::var("AWS_PROFILE").unwrap();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(exercise_profile(Path::new(&root), &profile));
        return;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("credential process {}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let result = std::panic::catch_unwind(|| {
        #[cfg(windows)]
        let command = {
            fs::write(root.join("process.ps1"), "param([string]$Root)\nif (Test-Path -LiteralPath (Join-Path $Root 'fail')) { exit 23 }\nGet-Content -LiteralPath (Join-Path $Root 'credentials.json') -Raw\n").unwrap();
            format!(
                "powershell.exe -NoProfile -NonInteractive -File \"{}\" \"{}\"",
                root.join("process.ps1").display(),
                root.display()
            )
        };
        #[cfg(not(windows))]
        let command = {
            fs::write(
                root.join("process.sh"),
                "#!/bin/sh\n[ ! -e \"$1/fail\" ] || exit 23\ncat \"$1/credentials.json\"\n",
            )
            .unwrap();
            format!(
                "sh '{}' '{}'",
                root.join("process.sh").display(),
                root.display()
            )
        };
        fs::write(root.join("config"), format!("[profile api]\nregion=eu-central-1\ncredential_process={command}\n[profile worker]\nregion=eu-central-1\ncredential_process={command}\n")).unwrap();
        fs::write(root.join("empty-credentials"), "").unwrap();
        for profile in ["api", "worker"] {
            let _ = fs::remove_file(root.join("fail"));
            let mut child = Command::new(std::env::current_exe().unwrap());
            child.args([
                "--exact",
                "profile_credentials_refresh_and_fail_closed",
                "--nocapture",
            ]);
            for (name, _) in std::env::vars_os() {
                if name.to_string_lossy().starts_with("AWS_") {
                    child.env_remove(name);
                }
            }
            let output = child
                .env("DAL_CREDENTIAL_TEST_CHILD", &root)
                .env("AWS_PROFILE", profile)
                .env("AWS_CONFIG_FILE", root.join("config"))
                .env(
                    "AWS_SHARED_CREDENTIALS_FILE",
                    root.join("empty-credentials"),
                )
                .env("AWS_EC2_METADATA_DISABLED", "true")
                .env("AWS_ACCESS_KEY_ID", "")
                .env("AWS_SECRET_ACCESS_KEY", "")
                .env("AWS_SESSION_TOKEN", "")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "Profile {profile}: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    });
    fs::remove_dir_all(&root).unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
