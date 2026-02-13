use crate::collector::{Collector, Metric, MetricSample, MetricType};
use crate::error::CollectorError;
use regex::Regex;
use std::fs;

const PROC_NET_DEV_PATH: &str = "/proc/net/dev";

/// Parsed network interface statistics.
#[derive(Debug, Clone)]
pub struct NetStats {
    pub interface: String,
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errors: u64,
    pub rx_drop: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errors: u64,
    pub tx_drop: u64,
}

/// Parse /proc/net/dev content into per-interface statistics.
pub fn parse_net_dev(content: &str) -> Result<Vec<NetStats>, CollectorError> {
    let mut stats = Vec::new();
    for line in content.lines() {
        // Skip header lines (they contain "|")
        if line.contains('|') || line.trim().is_empty() {
            continue;
        }
        let Some((iface, rest)) = line.split_once(':') else {
            continue;
        };
        let interface = iface.trim().to_string();
        let fields: Vec<&str> = rest.split_whitespace().collect();
        if fields.len() < 16 {
            return Err(CollectorError::Parse {
                path: PROC_NET_DEV_PATH.to_string(),
                field: format!("interface {}", interface),
                raw: line.to_string(),
            });
        }

        let parse_field =
            |idx: usize, field: &str, iface_name: &str| -> Result<u64, CollectorError> {
                fields[idx]
                    .parse::<u64>()
                    .map_err(|_| CollectorError::Parse {
                        path: PROC_NET_DEV_PATH.to_string(),
                        field: format!("{} for {}", field, iface_name),
                        raw: fields[idx].to_string(),
                    })
            };

        stats.push(NetStats {
            rx_bytes: parse_field(0, "rx_bytes", &interface)?,
            rx_packets: parse_field(1, "rx_packets", &interface)?,
            rx_errors: parse_field(2, "rx_errors", &interface)?,
            rx_drop: parse_field(3, "rx_drop", &interface)?,
            tx_bytes: parse_field(8, "tx_bytes", &interface)?,
            tx_packets: parse_field(9, "tx_packets", &interface)?,
            tx_errors: parse_field(10, "tx_errors", &interface)?,
            tx_drop: parse_field(11, "tx_drop", &interface)?,
            interface,
        });
    }
    Ok(stats)
}

pub struct NetworkCollector {
    exclude_pattern: Regex,
}

impl NetworkCollector {
    pub fn new(exclude_pattern: &str) -> Result<Self, regex::Error> {
        Ok(Self {
            exclude_pattern: Regex::new(exclude_pattern)?,
        })
    }
}

impl Collector for NetworkCollector {
    fn name(&self) -> &'static str {
        "network"
    }

    fn collect(&self) -> Result<Vec<Metric>, CollectorError> {
        let content =
            fs::read_to_string(PROC_NET_DEV_PATH).map_err(|e| CollectorError::FileRead {
                path: PROC_NET_DEV_PATH.to_string(),
                source: e,
            })?;
        self.collect_from_string(&content)
    }
}

impl NetworkCollector {
    pub fn collect_from_string(&self, content: &str) -> Result<Vec<Metric>, CollectorError> {
        let all_stats = parse_net_dev(content)?;
        let stats: Vec<&NetStats> = all_stats
            .iter()
            .filter(|s| !self.exclude_pattern.is_match(&s.interface))
            .collect();

        type MetricDef = (&'static str, &'static str, Box<dyn Fn(&NetStats) -> f64>);
        let metric_defs: Vec<MetricDef> = vec![
            (
                "sysmetrics_network_receive_bytes_total",
                "Total bytes received.",
                Box::new(|s: &NetStats| s.rx_bytes as f64),
            ),
            (
                "sysmetrics_network_transmit_bytes_total",
                "Total bytes transmitted.",
                Box::new(|s: &NetStats| s.tx_bytes as f64),
            ),
            (
                "sysmetrics_network_receive_packets_total",
                "Total packets received.",
                Box::new(|s: &NetStats| s.rx_packets as f64),
            ),
            (
                "sysmetrics_network_transmit_packets_total",
                "Total packets transmitted.",
                Box::new(|s: &NetStats| s.tx_packets as f64),
            ),
            (
                "sysmetrics_network_receive_errors_total",
                "Total receive errors.",
                Box::new(|s: &NetStats| s.rx_errors as f64),
            ),
            (
                "sysmetrics_network_transmit_errors_total",
                "Total transmit errors.",
                Box::new(|s: &NetStats| s.tx_errors as f64),
            ),
            (
                "sysmetrics_network_receive_drop_total",
                "Total receive drops.",
                Box::new(|s: &NetStats| s.rx_drop as f64),
            ),
            (
                "sysmetrics_network_transmit_drop_total",
                "Total transmit drops.",
                Box::new(|s: &NetStats| s.tx_drop as f64),
            ),
        ];

        let mut metrics = Vec::new();
        for (name, help, value_fn) in &metric_defs {
            let samples = stats
                .iter()
                .map(|s| MetricSample {
                    labels: vec![("interface".to_string(), s.interface.clone())],
                    value: value_fn(s),
                })
                .collect();
            metrics.push(Metric {
                name: name.to_string(),
                help: help.to_string(),
                metric_type: MetricType::Counter,
                samples,
            });
        }

        Ok(metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NET_DEV_FIXTURE: &str = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 1234567  12345    0    0    0     0          0         0  1234567  12345    0    0    0     0       0          0
  eth0: 9876543  98765    5    2    0     0          0         0  5432198  54321    1    3    0     0       0          0
";

    #[test]
    fn test_parse_net_dev() {
        let stats = parse_net_dev(NET_DEV_FIXTURE).unwrap();
        assert_eq!(stats.len(), 2);

        assert_eq!(stats[0].interface, "lo");
        assert_eq!(stats[0].rx_bytes, 1234567);
        assert_eq!(stats[0].rx_packets, 12345);
        assert_eq!(stats[0].tx_bytes, 1234567);

        assert_eq!(stats[1].interface, "eth0");
        assert_eq!(stats[1].rx_bytes, 9876543);
        assert_eq!(stats[1].rx_errors, 5);
        assert_eq!(stats[1].rx_drop, 2);
        assert_eq!(stats[1].tx_bytes, 5432198);
        assert_eq!(stats[1].tx_errors, 1);
        assert_eq!(stats[1].tx_drop, 3);
    }

    #[test]
    fn test_parse_net_dev_empty() {
        let input = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
";
        let stats = parse_net_dev(input).unwrap();
        assert!(stats.is_empty());
    }

    #[test]
    fn test_network_collector_filters_loopback() {
        let collector = NetworkCollector::new("^(lo|veth)").unwrap();
        let metrics = collector.collect_from_string(NET_DEV_FIXTURE).unwrap();
        // All metrics should only have eth0
        for metric in &metrics {
            assert_eq!(metric.samples.len(), 1);
            assert_eq!(metric.samples[0].labels[0].1, "eth0");
        }
    }

    #[test]
    fn test_network_collector_metric_values() {
        let collector = NetworkCollector::new("^lo$").unwrap();
        let metrics = collector.collect_from_string(NET_DEV_FIXTURE).unwrap();
        assert_eq!(metrics.len(), 8);

        // receive_bytes_total for eth0
        assert_eq!(metrics[0].name, "sysmetrics_network_receive_bytes_total");
        assert_eq!(metrics[0].samples[0].value, 9876543.0);

        // transmit_bytes_total for eth0
        assert_eq!(metrics[1].name, "sysmetrics_network_transmit_bytes_total");
        assert_eq!(metrics[1].samples[0].value, 5432198.0);
    }

    #[test]
    fn test_network_collector_interface_with_special_chars() {
        let input = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
docker0: 100 10 0 0 0 0 0 0 200 20 0 0 0 0 0 0
br-abc123: 300 30 0 0 0 0 0 0 400 40 0 0 0 0 0 0
";
        let collector = NetworkCollector::new("^$").unwrap(); // exclude nothing
        let metrics = collector.collect_from_string(input).unwrap();
        assert_eq!(metrics[0].samples.len(), 2);
        assert_eq!(metrics[0].samples[0].labels[0].1, "docker0");
        assert_eq!(metrics[0].samples[1].labels[0].1, "br-abc123");
    }
}
