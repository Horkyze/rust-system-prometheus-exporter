pub mod cpu;
pub mod disk;
pub mod memory;
pub mod network;

use crate::error::CollectorError;
use std::fmt;

/// Type of a Prometheus metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricType {
    Counter,
    Gauge,
}

impl fmt::Display for MetricType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetricType::Counter => write!(f, "counter"),
            MetricType::Gauge => write!(f, "gauge"),
        }
    }
}

/// A single sample within a metric family.
#[derive(Debug, Clone)]
pub struct MetricSample {
    pub labels: Vec<(String, String)>,
    pub value: f64,
}

/// A metric family with help text, type, and samples.
#[derive(Debug, Clone)]
pub struct Metric {
    pub name: String,
    pub help: String,
    pub metric_type: MetricType,
    pub samples: Vec<MetricSample>,
}

/// Trait that all metric collectors implement.
pub trait Collector: Send + Sync {
    /// Unique name for this collector (e.g., "cpu", "memory").
    fn name(&self) -> &'static str;

    /// Collect current metrics from the system.
    fn collect(&self) -> Result<Vec<Metric>, CollectorError>;
}

/// Registry holds all registered collectors and gathers metrics from them.
pub struct Registry {
    collectors: Vec<Box<dyn Collector>>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            collectors: Vec::new(),
        }
    }

    pub fn register(&mut self, collector: Box<dyn Collector>) {
        self.collectors.push(collector);
    }

    /// Collect metrics from all registered collectors.
    /// Returns per-collector results along with scrape metadata.
    pub fn collect_all(&self) -> Vec<CollectorResult> {
        self.collectors
            .iter()
            .map(|c| {
                let start = std::time::Instant::now();
                let result = c.collect();
                let duration = start.elapsed();
                CollectorResult {
                    name: c.name(),
                    duration,
                    result,
                }
            })
            .collect()
    }
}

/// Result of a single collector's scrape.
pub struct CollectorResult {
    pub name: &'static str,
    pub duration: std::time::Duration,
    pub result: Result<Vec<Metric>, CollectorError>,
}

/// Render a slice of metrics into Prometheus exposition format.
pub fn render_metrics(metrics: &[Metric]) -> String {
    let mut output = String::new();
    for metric in metrics {
        output.push_str(&format!("# HELP {} {}\n", metric.name, metric.help));
        output.push_str(&format!("# TYPE {} {}\n", metric.name, metric.metric_type));
        for sample in &metric.samples {
            output.push_str(&metric.name);
            if !sample.labels.is_empty() {
                output.push('{');
                for (i, (key, value)) in sample.labels.iter().enumerate() {
                    if i > 0 {
                        output.push(',');
                    }
                    output.push_str(&format!("{}=\"{}\"", key, escape_label_value(value)));
                }
                output.push('}');
            }
            output.push(' ');
            output.push_str(&format_float(sample.value));
            output.push('\n');
        }
    }
    output
}

/// Escape a Prometheus label value: backslash, double-quote, and newline.
fn escape_label_value(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            '\n' => result.push_str("\\n"),
            _ => result.push(c),
        }
    }
    result
}

/// Format a float for Prometheus output.
/// Integers are rendered without decimal point, others with minimal precision.
fn format_float(v: f64) -> String {
    if v.is_infinite() {
        if v.is_sign_positive() {
            return "+Inf".to_string();
        } else {
            return "-Inf".to_string();
        }
    }
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v == v.floor() && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        // Use enough precision to roundtrip
        let s = format!("{}", v);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_label_value_plain() {
        assert_eq!(escape_label_value("hello"), "hello");
    }

    #[test]
    fn test_escape_label_value_backslash() {
        assert_eq!(escape_label_value("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_escape_label_value_quote() {
        assert_eq!(escape_label_value("a\"b"), "a\\\"b");
    }

    #[test]
    fn test_escape_label_value_newline() {
        assert_eq!(escape_label_value("a\nb"), "a\\nb");
    }

    #[test]
    fn test_escape_label_value_combined() {
        assert_eq!(escape_label_value("a\\\"b\nc"), "a\\\\\\\"b\\nc");
    }

    #[test]
    fn test_format_float_integer() {
        assert_eq!(format_float(42.0), "42");
    }

    #[test]
    fn test_format_float_decimal() {
        assert_eq!(format_float(3.125), "3.125");
    }

    #[test]
    fn test_format_float_zero() {
        assert_eq!(format_float(0.0), "0");
    }

    #[test]
    fn test_format_float_inf() {
        assert_eq!(format_float(f64::INFINITY), "+Inf");
        assert_eq!(format_float(f64::NEG_INFINITY), "-Inf");
    }

    #[test]
    fn test_format_float_nan() {
        assert_eq!(format_float(f64::NAN), "NaN");
    }

    #[test]
    fn test_render_metrics_counter() {
        let metrics = vec![Metric {
            name: "sysmetrics_test_total".to_string(),
            help: "A test counter.".to_string(),
            metric_type: MetricType::Counter,
            samples: vec![
                MetricSample {
                    labels: vec![("mode".to_string(), "user".to_string())],
                    value: 123.0,
                },
                MetricSample {
                    labels: vec![("mode".to_string(), "system".to_string())],
                    value: 456.0,
                },
            ],
        }];
        let output = render_metrics(&metrics);
        assert!(output.contains("# HELP sysmetrics_test_total A test counter."));
        assert!(output.contains("# TYPE sysmetrics_test_total counter"));
        assert!(output.contains("sysmetrics_test_total{mode=\"user\"} 123"));
        assert!(output.contains("sysmetrics_test_total{mode=\"system\"} 456"));
    }

    #[test]
    fn test_render_metrics_gauge_no_labels() {
        let metrics = vec![Metric {
            name: "sysmetrics_cpu_count".to_string(),
            help: "Number of CPUs.".to_string(),
            metric_type: MetricType::Gauge,
            samples: vec![MetricSample {
                labels: vec![],
                value: 4.0,
            }],
        }];
        let output = render_metrics(&metrics);
        assert!(output.contains("# TYPE sysmetrics_cpu_count gauge"));
        assert!(output.contains("sysmetrics_cpu_count 4\n"));
    }

    #[test]
    fn test_render_metrics_label_escaping() {
        let metrics = vec![Metric {
            name: "sysmetrics_test".to_string(),
            help: "Test metric.".to_string(),
            metric_type: MetricType::Gauge,
            samples: vec![MetricSample {
                labels: vec![("path".to_string(), "/a\"b\\c\nd".to_string())],
                value: 1.0,
            }],
        }];
        let output = render_metrics(&metrics);
        assert!(output.contains("path=\"/a\\\"b\\\\c\\nd\""));
    }

    #[test]
    fn test_render_metrics_multiple_labels() {
        let metrics = vec![Metric {
            name: "sysmetrics_cpu_seconds_total".to_string(),
            help: "Total CPU time.".to_string(),
            metric_type: MetricType::Counter,
            samples: vec![MetricSample {
                labels: vec![
                    ("cpu".to_string(), "0".to_string()),
                    ("mode".to_string(), "user".to_string()),
                ],
                value: 185.39,
            }],
        }];
        let output = render_metrics(&metrics);
        assert!(output.contains("sysmetrics_cpu_seconds_total{cpu=\"0\",mode=\"user\"} 185.39"));
    }
}
