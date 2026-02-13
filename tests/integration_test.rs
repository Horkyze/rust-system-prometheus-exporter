use std::net::TcpListener;
use tokio::time::{sleep, Duration};

// We need to reference the library crate, but since this is a binary crate
// we test the server via HTTP requests.

/// Find an available port for testing.
fn find_available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Start the sysmetrics-rs server on the given port, returning a handle.
async fn start_server(port: u16) -> tokio::task::JoinHandle<()> {
    let handle = tokio::spawn(async move {
        let child = tokio::process::Command::new("cargo")
            .args(["run", "--", "--listen", &format!("127.0.0.1:{}", port)])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match child {
            Ok(mut c) => {
                let _ = c.wait().await;
            }
            Err(e) => {
                eprintln!("Failed to start server: {}", e);
            }
        }
    });

    // Wait for the server to start
    for _ in 0..50 {
        sleep(Duration::from_millis(200)).await;
        if let Ok(_) = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
            return handle;
        }
    }
    handle
}

#[tokio::test]
async fn test_health_endpoint() {
    let port = find_available_port();
    let server_handle = start_server(port).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(&format!("http://127.0.0.1:{}/health", port))
        .send()
        .await;

    if let Ok(resp) = resp {
        assert_eq!(resp.status(), 200);
    }

    server_handle.abort();
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let port = find_available_port();
    let server_handle = start_server(port).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(&format!("http://127.0.0.1:{}/metrics", port))
        .send()
        .await;

    if let Ok(resp) = resp {
        assert_eq!(resp.status(), 200);
        let content_type = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            content_type.contains("text/plain"),
            "Expected text/plain content type, got: {}",
            content_type
        );

        let body = resp.text().await.unwrap();

        // Should contain HELP and TYPE lines
        assert!(body.contains("# HELP"), "Missing HELP lines");
        assert!(body.contains("# TYPE"), "Missing TYPE lines");

        // Should contain build info
        assert!(
            body.contains("sysmetrics_build_info"),
            "Missing build_info metric"
        );

        // Should contain scrape duration
        assert!(
            body.contains("sysmetrics_scrape_duration_seconds"),
            "Missing scrape_duration metric"
        );

        // On a real Linux system, should contain CPU metrics
        if std::path::Path::new("/proc/stat").exists() {
            assert!(
                body.contains("sysmetrics_cpu_seconds_total"),
                "Missing CPU metrics"
            );
            assert!(
                body.contains("sysmetrics_cpu_count"),
                "Missing cpu_count metric"
            );
        }

        // On a real Linux system, should contain memory metrics
        if std::path::Path::new("/proc/meminfo").exists() {
            assert!(
                body.contains("sysmetrics_memory_total_bytes"),
                "Missing memory metrics"
            );
        }
    }

    server_handle.abort();
}

#[tokio::test]
async fn test_index_endpoint() {
    let port = find_available_port();
    let server_handle = start_server(port).await;

    let client = reqwest::Client::new();
    let resp = client
        .get(&format!("http://127.0.0.1:{}/", port))
        .send()
        .await;

    if let Ok(resp) = resp {
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(
            body.contains("/metrics"),
            "Index page should link to /metrics"
        );
    }

    server_handle.abort();
}
