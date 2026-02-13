use crate::collector::{Collector, Metric, MetricSample, MetricType};
use crate::error::CollectorError;
use regex::Regex;
use std::fs;

const PROC_DISKSTATS_PATH: &str = "/proc/diskstats";
const SECTOR_SIZE: f64 = 512.0;

/// Parsed disk statistics for a single device.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct DiskStats {
    pub device: String,
    pub reads_completed: u64,
    pub reads_merged: u64,
    pub sectors_read: u64,
    pub time_reading_ms: u64,
    pub writes_completed: u64,
    pub writes_merged: u64,
    pub sectors_written: u64,
    pub time_writing_ms: u64,
    pub ios_in_progress: u64,
    pub time_doing_ios_ms: u64,
    pub weighted_time_ms: u64,
}

/// Parse /proc/diskstats content into a list of disk statistics.
pub fn parse_diskstats(content: &str) -> Result<Vec<DiskStats>, CollectorError> {
    let mut stats = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 14 {
            continue; // Skip lines that don't have enough fields
        }
        let device = parts[2].to_string();

        let parse_field = |idx: usize, field: &str, dev: &str| -> Result<u64, CollectorError> {
            parts[idx]
                .parse::<u64>()
                .map_err(|_| CollectorError::Parse {
                    path: PROC_DISKSTATS_PATH.to_string(),
                    field: format!("{} for {}", field, dev),
                    raw: parts[idx].to_string(),
                })
        };

        stats.push(DiskStats {
            reads_completed: parse_field(3, "reads_completed", &device)?,
            reads_merged: parse_field(4, "reads_merged", &device)?,
            sectors_read: parse_field(5, "sectors_read", &device)?,
            time_reading_ms: parse_field(6, "time_reading_ms", &device)?,
            writes_completed: parse_field(7, "writes_completed", &device)?,
            writes_merged: parse_field(8, "writes_merged", &device)?,
            sectors_written: parse_field(9, "sectors_written", &device)?,
            time_writing_ms: parse_field(10, "time_writing_ms", &device)?,
            ios_in_progress: parse_field(11, "ios_in_progress", &device)?,
            time_doing_ios_ms: parse_field(12, "time_doing_ios_ms", &device)?,
            weighted_time_ms: parse_field(13, "weighted_time_ms", &device)?,
            device,
        });
    }
    Ok(stats)
}

pub struct DiskCollector {
    exclude_pattern: Regex,
}

impl DiskCollector {
    pub fn new(exclude_pattern: &str) -> Result<Self, regex::Error> {
        Ok(Self {
            exclude_pattern: Regex::new(exclude_pattern)?,
        })
    }
}

impl Collector for DiskCollector {
    fn name(&self) -> &'static str {
        "disk"
    }

    fn collect(&self) -> Result<Vec<Metric>, CollectorError> {
        let content =
            fs::read_to_string(PROC_DISKSTATS_PATH).map_err(|e| CollectorError::FileRead {
                path: PROC_DISKSTATS_PATH.to_string(),
                source: e,
            })?;
        self.collect_from_string(&content)
    }
}

impl DiskCollector {
    pub fn collect_from_string(&self, content: &str) -> Result<Vec<Metric>, CollectorError> {
        let all_stats = parse_diskstats(content)?;
        let stats: Vec<&DiskStats> = all_stats
            .iter()
            .filter(|s| !self.exclude_pattern.is_match(&s.device))
            .collect();

        type MetricDef = (
            &'static str,
            &'static str,
            MetricType,
            Box<dyn Fn(&DiskStats) -> f64>,
        );
        let metric_defs: Vec<MetricDef> = vec![
            (
                "sysmetrics_disk_reads_completed_total",
                "Total number of reads completed.",
                MetricType::Counter,
                Box::new(|s: &DiskStats| s.reads_completed as f64),
            ),
            (
                "sysmetrics_disk_writes_completed_total",
                "Total number of writes completed.",
                MetricType::Counter,
                Box::new(|s: &DiskStats| s.writes_completed as f64),
            ),
            (
                "sysmetrics_disk_read_bytes_total",
                "Total bytes read from disk.",
                MetricType::Counter,
                Box::new(|s: &DiskStats| s.sectors_read as f64 * SECTOR_SIZE),
            ),
            (
                "sysmetrics_disk_written_bytes_total",
                "Total bytes written to disk.",
                MetricType::Counter,
                Box::new(|s: &DiskStats| s.sectors_written as f64 * SECTOR_SIZE),
            ),
            (
                "sysmetrics_disk_io_time_seconds_total",
                "Total time spent doing I/Os in seconds.",
                MetricType::Counter,
                Box::new(|s: &DiskStats| s.time_doing_ios_ms as f64 / 1000.0),
            ),
            (
                "sysmetrics_disk_io_in_progress",
                "Number of I/Os currently in progress.",
                MetricType::Gauge,
                Box::new(|s: &DiskStats| s.ios_in_progress as f64),
            ),
        ];

        let mut metrics = Vec::new();
        for (name, help, metric_type, value_fn) in &metric_defs {
            let samples = stats
                .iter()
                .map(|s| MetricSample {
                    labels: vec![("device".to_string(), s.device.clone())],
                    value: value_fn(s),
                })
                .collect();
            metrics.push(Metric {
                name: name.to_string(),
                help: help.to_string(),
                metric_type: *metric_type,
                samples,
            });
        }

        Ok(metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISKSTATS_FIXTURE: &str = "\
   8       0 sda 12345 100 98765 4567 54321 200 87654 3456 5 6789 12345
   8       1 sda1 1234 10 9876 456 5432 20 8765 345 0 678 1234
   7       0 loop0 100 0 200 10 0 0 0 0 0 10 10
   1       0 ram0 0 0 0 0 0 0 0 0 0 0 0
 253       0 dm-0 5000 0 40000 2000 3000 0 24000 1000 2 3000 5000
";

    #[test]
    fn test_parse_diskstats() {
        let stats = parse_diskstats(DISKSTATS_FIXTURE).unwrap();
        assert_eq!(stats.len(), 5);
        assert_eq!(stats[0].device, "sda");
        assert_eq!(stats[0].reads_completed, 12345);
        assert_eq!(stats[0].sectors_read, 98765);
        assert_eq!(stats[0].writes_completed, 54321);
        assert_eq!(stats[0].sectors_written, 87654);
        assert_eq!(stats[0].ios_in_progress, 5);
    }

    #[test]
    fn test_parse_diskstats_empty() {
        let stats = parse_diskstats("").unwrap();
        assert!(stats.is_empty());
    }

    #[test]
    fn test_disk_collector_filters_virtual_devices() {
        let collector = DiskCollector::new("^(loop|ram|dm-)").unwrap();
        let metrics = collector.collect_from_string(DISKSTATS_FIXTURE).unwrap();
        // Should only have sda and sda1 (loop0, ram0, dm-0 filtered)
        let reads_metric = &metrics[0];
        assert_eq!(reads_metric.samples.len(), 2);
        assert_eq!(reads_metric.samples[0].labels[0].1, "sda");
        assert_eq!(reads_metric.samples[1].labels[0].1, "sda1");
    }

    #[test]
    fn test_disk_collector_metric_values() {
        let collector = DiskCollector::new("^(loop|ram|dm-)").unwrap();
        let metrics = collector.collect_from_string(DISKSTATS_FIXTURE).unwrap();

        // reads_completed_total for sda = 12345
        assert_eq!(metrics[0].samples[0].value, 12345.0);
        // read_bytes_total for sda = 98765 * 512
        assert_eq!(metrics[2].samples[0].value, 98765.0 * 512.0);
        // io_time_seconds_total for sda = 6789 / 1000
        assert!((metrics[4].samples[0].value - 6.789).abs() < 0.001);
    }

    #[test]
    fn test_disk_collector_device_with_hyphen() {
        let input = "   8       0 nvme0n1 1000 0 2000 100 500 0 1000 50 1 150 200\n";
        let collector = DiskCollector::new("^(loop|ram|dm-)").unwrap();
        let metrics = collector.collect_from_string(input).unwrap();
        assert_eq!(metrics[0].samples[0].labels[0].1, "nvme0n1");
    }
}
