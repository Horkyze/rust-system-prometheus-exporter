use crate::collector::{Collector, Metric, MetricSample, MetricType};
use crate::error::CollectorError;
use std::fs;

const PROC_STAT_PATH: &str = "/proc/stat";
const USER_HZ: f64 = 100.0;

const CPU_MODES: &[&str] = &[
    "user", "nice", "system", "idle", "iowait", "irq", "softirq", "steal",
];

/// Parsed CPU statistics for a single core.
#[derive(Debug, Clone)]
pub struct CpuStats {
    pub cpu_id: String,
    pub values: Vec<u64>,
}

/// Parse /proc/stat content into per-CPU statistics.
pub fn parse_cpu_stats(content: &str) -> Result<Vec<CpuStats>, CollectorError> {
    let mut stats = Vec::new();
    for line in content.lines() {
        // Match lines like "cpu0 ..." but not the aggregate "cpu ..." line
        if line.starts_with("cpu") && !line.starts_with("cpu ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 9 {
                return Err(CollectorError::Parse {
                    path: PROC_STAT_PATH.to_string(),
                    field: "cpu line".to_string(),
                    raw: line.to_string(),
                });
            }
            let cpu_id = parts[0]
                .strip_prefix("cpu")
                .ok_or_else(|| CollectorError::Parse {
                    path: PROC_STAT_PATH.to_string(),
                    field: "cpu id".to_string(),
                    raw: parts[0].to_string(),
                })?
                .to_string();

            let mut values = Vec::new();
            for (i, part) in parts[1..].iter().enumerate().take(8) {
                let v = part.parse::<u64>().map_err(|_| CollectorError::Parse {
                    path: PROC_STAT_PATH.to_string(),
                    field: format!("cpu{} column {}", cpu_id, i),
                    raw: part.to_string(),
                })?;
                values.push(v);
            }
            stats.push(CpuStats { cpu_id, values });
        }
    }
    if stats.is_empty() {
        return Err(CollectorError::Parse {
            path: PROC_STAT_PATH.to_string(),
            field: "cpu lines".to_string(),
            raw: "no per-cpu lines found".to_string(),
        });
    }
    Ok(stats)
}

pub struct CpuCollector;

impl Collector for CpuCollector {
    fn name(&self) -> &'static str {
        "cpu"
    }

    fn collect(&self) -> Result<Vec<Metric>, CollectorError> {
        let content = fs::read_to_string(PROC_STAT_PATH).map_err(|e| CollectorError::FileRead {
            path: PROC_STAT_PATH.to_string(),
            source: e,
        })?;
        self.collect_from_string(&content)
    }
}

impl CpuCollector {
    pub fn collect_from_string(&self, content: &str) -> Result<Vec<Metric>, CollectorError> {
        let stats = parse_cpu_stats(content)?;
        let cpu_count = stats.len();

        let mut samples = Vec::new();
        for stat in &stats {
            for (i, mode) in CPU_MODES.iter().enumerate() {
                if i < stat.values.len() {
                    samples.push(MetricSample {
                        labels: vec![
                            ("cpu".to_string(), stat.cpu_id.clone()),
                            ("mode".to_string(), mode.to_string()),
                        ],
                        value: stat.values[i] as f64 / USER_HZ,
                    });
                }
            }
        }

        Ok(vec![
            Metric {
                name: "sysmetrics_cpu_seconds_total".to_string(),
                help: "Total CPU time spent in each mode.".to_string(),
                metric_type: MetricType::Counter,
                samples,
            },
            Metric {
                name: "sysmetrics_cpu_count".to_string(),
                help: "Number of logical CPUs.".to_string(),
                metric_type: MetricType::Gauge,
                samples: vec![MetricSample {
                    labels: vec![],
                    value: cpu_count as f64,
                }],
            },
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROC_STAT_FIXTURE: &str = "\
cpu  74156 1260 22706 6316498 4539 0 456 0 0 0
cpu0 18539 315 5676 1579124 1134 0 114 0 0 0
cpu1 18540 315 5677 1579125 1135 0 114 0 0 0
intr 12345678
";

    const PROC_STAT_SINGLE_CPU: &str = "\
cpu  10000 200 3000 500000 100 0 50 0 0 0
cpu0 10000 200 3000 500000 100 0 50 0 0 0
";

    const PROC_STAT_128_CPUS: &str = include_str!("../../tests/fixtures/proc_stat_128.txt");

    #[test]
    fn test_parse_cpu_stats_two_cores() {
        let stats = parse_cpu_stats(PROC_STAT_FIXTURE).unwrap();
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].cpu_id, "0");
        assert_eq!(stats[0].values[0], 18539); // user
        assert_eq!(stats[0].values[2], 5676); // system
        assert_eq!(stats[1].cpu_id, "1");
        assert_eq!(stats[1].values[0], 18540);
    }

    #[test]
    fn test_parse_cpu_stats_single_core() {
        let stats = parse_cpu_stats(PROC_STAT_SINGLE_CPU).unwrap();
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].cpu_id, "0");
        assert_eq!(stats[0].values[0], 10000);
    }

    #[test]
    fn test_parse_cpu_stats_128_cores() {
        let stats = parse_cpu_stats(PROC_STAT_128_CPUS).unwrap();
        assert_eq!(stats.len(), 128);
        assert_eq!(stats[127].cpu_id, "127");
    }

    #[test]
    fn test_parse_cpu_stats_malformed() {
        let result = parse_cpu_stats("garbage data");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_cpu_stats_empty() {
        let result = parse_cpu_stats("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_cpu_stats_truncated_line() {
        let input = "cpu  100 200\ncpu0 100\n";
        let result = parse_cpu_stats(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_cpu_collector_metrics() {
        let collector = CpuCollector;
        let metrics = collector.collect_from_string(PROC_STAT_FIXTURE).unwrap();
        assert_eq!(metrics.len(), 2);

        let cpu_seconds = &metrics[0];
        assert_eq!(cpu_seconds.name, "sysmetrics_cpu_seconds_total");
        assert_eq!(cpu_seconds.metric_type, MetricType::Counter);
        // 2 CPUs * 8 modes = 16 samples
        assert_eq!(cpu_seconds.samples.len(), 16);

        // Check user time for cpu0: 18539 / 100 = 185.39
        let cpu0_user = &cpu_seconds.samples[0];
        assert_eq!(cpu0_user.labels[0], ("cpu".to_string(), "0".to_string()));
        assert_eq!(
            cpu0_user.labels[1],
            ("mode".to_string(), "user".to_string())
        );
        assert!((cpu0_user.value - 185.39).abs() < 0.001);

        let cpu_count = &metrics[1];
        assert_eq!(cpu_count.name, "sysmetrics_cpu_count");
        assert_eq!(cpu_count.metric_type, MetricType::Gauge);
        assert_eq!(cpu_count.samples[0].value, 2.0);
    }
}
