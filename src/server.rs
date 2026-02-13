use crate::collector::{render_metrics, Metric, MetricSample, MetricType, Registry};
use axum::{extract::State, http::StatusCode, response::Html, routing::get, Router};
use std::sync::Arc;
use std::time::Instant;

/// Shared application state.
pub struct AppState {
    pub registry: Registry,
    pub version: &'static str,
    pub rustc_version: &'static str,
}

/// Build the axum router with all routes.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .with_state(state)
}

async fn index_handler() -> Html<&'static str> {
    Html(
        "<html><head><title>sysmetrics-rs</title></head><body>\
         <h1>sysmetrics-rs</h1>\
         <p><a href=\"/metrics\">Metrics</a></p>\
         </body></html>",
    )
}

async fn health_handler() -> StatusCode {
    StatusCode::OK
}

async fn metrics_handler(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, [(String, String); 1], String) {
    let scrape_start = Instant::now();
    let results = state.registry.collect_all();
    let total_duration = scrape_start.elapsed();

    let mut all_metrics: Vec<Metric> = Vec::new();
    let mut meta_metrics: Vec<Metric> = Vec::new();

    // Per-collector scrape duration and success metrics
    let mut duration_samples = Vec::new();
    let mut success_samples = Vec::new();
    let mut error_samples = Vec::new();

    for result in &results {
        let collector_name = result.name;
        let duration_secs = result.duration.as_secs_f64();

        duration_samples.push(MetricSample {
            labels: vec![("collector".to_string(), collector_name.to_string())],
            value: duration_secs,
        });

        match &result.result {
            Ok(metrics) => {
                success_samples.push(MetricSample {
                    labels: vec![("collector".to_string(), collector_name.to_string())],
                    value: 1.0,
                });
                error_samples.push(MetricSample {
                    labels: vec![("collector".to_string(), collector_name.to_string())],
                    value: 0.0,
                });
                all_metrics.extend(metrics.clone());
            }
            Err(e) => {
                tracing::error!(collector = collector_name, error = %e, "collector failed");
                success_samples.push(MetricSample {
                    labels: vec![("collector".to_string(), collector_name.to_string())],
                    value: 0.0,
                });
                error_samples.push(MetricSample {
                    labels: vec![("collector".to_string(), collector_name.to_string())],
                    value: 1.0,
                });
            }
        }
    }

    // Add meta-metrics
    meta_metrics.push(Metric {
        name: "sysmetrics_scrape_duration_seconds".to_string(),
        help: "Duration of collector scrape in seconds.".to_string(),
        metric_type: MetricType::Gauge,
        samples: duration_samples,
    });

    meta_metrics.push(Metric {
        name: "sysmetrics_scrape_duration_seconds_total".to_string(),
        help: "Total scrape duration in seconds.".to_string(),
        metric_type: MetricType::Gauge,
        samples: vec![MetricSample {
            labels: vec![],
            value: total_duration.as_secs_f64(),
        }],
    });

    meta_metrics.push(Metric {
        name: "sysmetrics_collector_success".to_string(),
        help: "Whether the collector succeeded (1) or failed (0).".to_string(),
        metric_type: MetricType::Gauge,
        samples: success_samples,
    });

    meta_metrics.push(Metric {
        name: "sysmetrics_collector_errors_total".to_string(),
        help: "Total collector scrape errors.".to_string(),
        metric_type: MetricType::Counter,
        samples: error_samples,
    });

    meta_metrics.push(Metric {
        name: "sysmetrics_build_info".to_string(),
        help: "Build information for sysmetrics-rs.".to_string(),
        metric_type: MetricType::Gauge,
        samples: vec![MetricSample {
            labels: vec![
                ("version".to_string(), state.version.to_string()),
                ("rustc".to_string(), state.rustc_version.to_string()),
            ],
            value: 1.0,
        }],
    });

    all_metrics.extend(meta_metrics);

    let body = render_metrics(&all_metrics);
    let content_type = "text/plain; version=0.0.4; charset=utf-8".to_string();
    (
        StatusCode::OK,
        [("content-type".to_string(), content_type)],
        body,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::Registry;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_state() -> Arc<AppState> {
        Arc::new(AppState {
            registry: Registry::new(),
            version: "0.1.0-test",
            rustc_version: "test",
        })
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_index_endpoint() {
        let app = build_router(test_state());
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert!(body_str.contains("/metrics"));
    }

    #[tokio::test]
    async fn test_metrics_endpoint_empty_registry() {
        let app = build_router(test_state());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let content_type = response
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(content_type.contains("text/plain"));

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        // Should contain meta-metrics even with no collectors
        assert!(body_str.contains("sysmetrics_build_info"));
        assert!(body_str.contains("sysmetrics_scrape_duration_seconds_total"));
    }
}
