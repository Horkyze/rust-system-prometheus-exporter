use crate::collector::{Collector, Metric, MetricSample, MetricType};
use crate::error::CollectorError;
use std::collections::HashMap;
use std::fs;

const PROC_MEMINFO_PATH: &str = "/proc/meminfo";

/// Parse /proc/meminfo content into a map of field name -> value in kB.
pub fn parse_meminfo(content: &str) -> Result<HashMap<String, u64>, CollectorError> {
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Format: "FieldName:     12345 kB"
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let rest = rest.trim();
        let value_str = rest
            .split_whitespace()
            .next()
            .ok_or_else(|| CollectorError::Parse {
                path: PROC_MEMINFO_PATH.to_string(),
                field: key.to_string(),
                raw: line.to_string(),
            })?;
        let value = value_str
            .parse::<u64>()
            .map_err(|_| CollectorError::Parse {
                path: PROC_MEMINFO_PATH.to_string(),
                field: key.to_string(),
                raw: value_str.to_string(),
            })?;
        map.insert(key.to_string(), value);
    }
    Ok(map)
}

fn get_field(map: &HashMap<String, u64>, field: &str) -> Result<u64, CollectorError> {
    map.get(field)
        .copied()
        .ok_or_else(|| CollectorError::Parse {
            path: PROC_MEMINFO_PATH.to_string(),
            field: field.to_string(),
            raw: "field not found".to_string(),
        })
}

pub struct MemoryCollector;

impl Collector for MemoryCollector {
    fn name(&self) -> &'static str {
        "memory"
    }

    fn collect(&self) -> Result<Vec<Metric>, CollectorError> {
        let content =
            fs::read_to_string(PROC_MEMINFO_PATH).map_err(|e| CollectorError::FileRead {
                path: PROC_MEMINFO_PATH.to_string(),
                source: e,
            })?;
        self.collect_from_string(&content)
    }
}

impl MemoryCollector {
    pub fn collect_from_string(&self, content: &str) -> Result<Vec<Metric>, CollectorError> {
        let map = parse_meminfo(content)?;

        let total = get_field(&map, "MemTotal")?;
        let free = get_field(&map, "MemFree")?;
        let available = get_field(&map, "MemAvailable")?;
        let buffers = get_field(&map, "Buffers")?;
        let cached = get_field(&map, "Cached")?;
        let swap_total = get_field(&map, "SwapTotal")?;
        let swap_free = get_field(&map, "SwapFree")?;

        let used = total
            .saturating_sub(free)
            .saturating_sub(buffers)
            .saturating_sub(cached);

        let kb_to_bytes = 1024.0;

        let metrics = vec![
            (
                "sysmetrics_memory_total_bytes",
                "Total memory in bytes.",
                total,
            ),
            (
                "sysmetrics_memory_free_bytes",
                "Free memory in bytes.",
                free,
            ),
            (
                "sysmetrics_memory_available_bytes",
                "Available memory in bytes.",
                available,
            ),
            (
                "sysmetrics_memory_buffers_bytes",
                "Buffer memory in bytes.",
                buffers,
            ),
            (
                "sysmetrics_memory_cached_bytes",
                "Cached memory in bytes.",
                cached,
            ),
            (
                "sysmetrics_memory_swap_total_bytes",
                "Total swap in bytes.",
                swap_total,
            ),
            (
                "sysmetrics_memory_swap_free_bytes",
                "Free swap in bytes.",
                swap_free,
            ),
            (
                "sysmetrics_memory_used_bytes",
                "Used memory in bytes (total - free - buffers - cached).",
                used,
            ),
        ];

        Ok(metrics
            .into_iter()
            .map(|(name, help, value_kb)| Metric {
                name: name.to_string(),
                help: help.to_string(),
                metric_type: MetricType::Gauge,
                samples: vec![MetricSample {
                    labels: vec![],
                    value: value_kb as f64 * kb_to_bytes,
                }],
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEMINFO_FIXTURE: &str = "\
MemTotal:       16384000 kB
MemFree:         1234567 kB
MemAvailable:    8765432 kB
Buffers:          234567 kB
Cached:          3456789 kB
SwapCached:        12345 kB
SwapTotal:       4194304 kB
SwapFree:        4194000 kB
";

    #[test]
    fn test_parse_meminfo() {
        let map = parse_meminfo(MEMINFO_FIXTURE).unwrap();
        assert_eq!(map["MemTotal"], 16384000);
        assert_eq!(map["MemFree"], 1234567);
        assert_eq!(map["MemAvailable"], 8765432);
        assert_eq!(map["Buffers"], 234567);
        assert_eq!(map["Cached"], 3456789);
        assert_eq!(map["SwapTotal"], 4194304);
        assert_eq!(map["SwapFree"], 4194000);
    }

    #[test]
    fn test_parse_meminfo_empty() {
        let map = parse_meminfo("").unwrap();
        assert!(map.is_empty());
    }

    #[test]
    fn test_parse_meminfo_malformed_value() {
        let input = "MemTotal:       abc kB\n";
        let result = parse_meminfo(input);
        assert!(result.is_err());
    }

    #[test]
    fn test_memory_collector_metrics() {
        let collector = MemoryCollector;
        let metrics = collector.collect_from_string(MEMINFO_FIXTURE).unwrap();
        assert_eq!(metrics.len(), 8);

        // Check total: 16384000 kB * 1024 = 16777216000 bytes
        let total = &metrics[0];
        assert_eq!(total.name, "sysmetrics_memory_total_bytes");
        assert_eq!(total.metric_type, MetricType::Gauge);
        assert!((total.samples[0].value - 16384000.0 * 1024.0).abs() < 1.0);

        // Check used: (16384000 - 1234567 - 234567 - 3456789) * 1024
        let used = &metrics[7];
        assert_eq!(used.name, "sysmetrics_memory_used_bytes");
        let expected_used = (16384000u64 - 1234567 - 234567 - 3456789) as f64 * 1024.0;
        assert!((used.samples[0].value - expected_used).abs() < 1.0);
    }

    #[test]
    fn test_memory_collector_missing_field() {
        let input = "MemTotal: 16384000 kB\nMemFree: 1234567 kB\n";
        let collector = MemoryCollector;
        let result = collector.collect_from_string(input);
        assert!(result.is_err());
    }
}
