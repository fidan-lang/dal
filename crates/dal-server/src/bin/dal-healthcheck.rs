//! Internal container probe: no provider initialization, credentials or curl dependency.
use anyhow::{Context, Result, bail, ensure};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

fn probe_url(listen_addr: &str, ready: bool) -> Result<reqwest::Url> {
    let listen: SocketAddr = listen_addr.parse().context("invalid DAL_LISTEN_ADDR")?;
    let localhost = if listen.is_ipv4() {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        IpAddr::V6(Ipv6Addr::LOCALHOST)
    };
    let address = SocketAddr::new(localhost, listen.port());
    let path = if ready { "readyz" } else { "health" };
    Ok(format!("http://{address}/{path}").parse()?)
}

async fn check(url: reqwest::Url) -> Result<()> {
    let response = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PROBE_TIMEOUT)
        .build()?
        .get(url)
        .send()
        .await?;
    ensure!(
        response.status().is_success(),
        "probe returned HTTP {}",
        response.status().as_u16()
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let ready = match arguments.next().as_deref() {
        None => false,
        Some("--ready") => true,
        _ => bail!("usage: dal-healthcheck [--ready]"),
    };
    ensure!(
        arguments.next().is_none(),
        "usage: dal-healthcheck [--ready]"
    );
    let listen = std::env::var("DAL_LISTEN_ADDR")
        .unwrap_or_else(|_| dal_server::config::DEFAULT_LISTEN_ADDR.to_owned());
    check(probe_url(&listen, ready)?).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn probe_targets_only_loopback_with_the_configured_port() {
        assert_eq!(
            probe_url("0.0.0.0:8080", false).unwrap().as_str(),
            "http://127.0.0.1:8080/health"
        );
        assert_eq!(
            probe_url("192.0.2.1:9000", true).unwrap().as_str(),
            "http://127.0.0.1:9000/readyz"
        );
        assert_eq!(
            probe_url("[::]:9000", false).unwrap().as_str(),
            "http://[::1]:9000/health"
        );
        assert!(probe_url("https://external.example", false).is_err());
    }

    #[tokio::test]
    async fn probe_accepts_success_and_rejects_redirects_and_failure() {
        for status in [200, 307, 503] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 1024];
                let len = stream.read(&mut request).await.unwrap();
                assert!(
                    std::str::from_utf8(&request[..len])
                        .unwrap()
                        .starts_with("GET /health HTTP/1.1")
                );
                stream.write_all(format!("HTTP/1.1 {status} fixture\r\nContent-Length: 0\r\nLocation: http://127.0.0.1:9/\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            });
            let result = check(probe_url(&address.to_string(), false).unwrap()).await;
            server.await.unwrap();
            if status == 200 {
                result.unwrap();
            } else {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains(&format!("HTTP {status}"))
                );
            }
        }
    }
    #[tokio::test]
    async fn probe_aborts_a_stalled_local_response() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release, stopped) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (connection, _) = listener.accept().await.unwrap();
            let _ = stopped.await;
            drop(connection);
        });
        let error = check(probe_url(&address.to_string(), false).unwrap())
            .await
            .unwrap_err();
        assert!(error.downcast_ref::<reqwest::Error>().unwrap().is_timeout());
        let _ = release.send(());
        server.await.unwrap();
    }
}
