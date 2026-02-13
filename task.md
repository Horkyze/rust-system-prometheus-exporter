# `sysmetrics-rs` — A Linux System Metrics Prometheus Exporter in Rust

## Project overview

A lightweight, async HTTP server that reads system metrics directly from the Linux `/proc` and `/sys` filesystems and exposes them in Prometheus exposition format at `/metrics`. No C bindings, no `libprocps` — just raw file parsing, the way Oxide would do it.

The goal is not to replace `node_exporter`. It's to demonstrate that you can write clean, idiomatic, production-grade Rust that touches real systems internals and speaks the observability language fluently.

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│                  sysmetrics-rs                   │
│                                                  │
│  ┌──────────┐   ┌────────────┐   ┌───────────┐  │
│  │ Collector │──▶│  Registry  │──▶│ HTTP/metrics│ │
│  │ modules   │   │ (in-memory)│   │  (axum)    │ │
│  └──────────┘   └────────────┘   └───────────┘  │
│       │                                │         │
│  ┌────▼─────┐                   ┌──────▼──────┐  │
│  │ /proc/*  │                   │ Prometheus  │  │
│  │ /sys/*   │                   │ text format │  │
│  └──────────┘                   └─────────────┘  │
└─────────────────────────────────────────────────┘
```

The design follows a **collector pattern**: each metric domain (CPU, memory, disk, network) is an independent module that implements a shared `Collector` trait. A central registry calls all collectors on each scrape request and assembles the output. No background polling — metrics are read fresh on every `/metrics` hit, just like `node_exporter` does.

---

## Phase 1 — Skeleton & CPU metrics

**Goal:** Get a working HTTP server that returns at least one real metric.

### Tasks

1. **Project scaffold**
   - `cargo init sysmetrics-rs`
   - Set up `Cargo.toml` with: `axum`, `tokio` (full features), `serde`, `clap`, `tracing`, `tracing-subscriber`, `anyhow`, `thiserror`
   - Create module structure:
     ```
     src/
       main.rs
       config.rs
       server.rs
       error.rs
       collector/
         mod.rs        // Collector trait + registry
         cpu.rs
         memory.rs
         disk.rs
         network.rs
     ```
   - Add a basic `.gitignore`, `README.md`, and MIT license

2. **Define the `Collector` trait**
   ```rust
   pub trait Collector: Send + Sync {
       /// Unique prefix for this collector's metrics (e.g., "cpu", "memory")
       fn name(&self) -> &'static str;

       /// Collect current metrics. Returns a vec of Metric structs.
       fn collect(&self) -> Result<Vec<Metric>, CollectorError>;
   }
   ```

3. **Define the `Metric` type**
   ```rust
   pub enum MetricType {
       Counter,
       Gauge,
   }

   pub struct MetricSample {
       pub labels: Vec<(String, String)>,
       pub value: f64,
   }

   pub struct Metric {
       pub name: String,
       pub help: String,
       pub metric_type: MetricType,
       pub samples: Vec<MetricSample>,
   }
   ```

4. **Implement Prometheus text format renderer**
   Write a function `fn render_metrics(metrics: &[Metric]) -> String` that produces valid Prometheus exposition format:
   ```
   # HELP sysmetrics_cpu_seconds_total Total CPU time spent in each mode.
   # TYPE sysmetrics_cpu_seconds_total counter
   sysmetrics_cpu_seconds_total{cpu="0",mode="user"} 12345.67
   sysmetrics_cpu_seconds_total{cpu="0",mode="system"} 6789.01
   ```
   Rules to follow:
   - `# HELP` line with metric description
   - `# TYPE` line with counter/gauge
   - Label pairs in `{key="value"}` format, properly escaped
   - Metric name prefix: `sysmetrics_`
   - Values as float with no trailing zeros beyond necessity

5. **Implement CPU collector** — parse `/proc/stat`
   File format (first few lines):
   ```
   cpu  74156 1260 22706 6316498 4539 0 456 0 0 0
   cpu0 18539 315 5676 1579124 1134 0 114 0 0 0
   cpu1 18540 315 5677 1579125 1135 0 114 0 0 0
   ```
   Columns after `cpuN`: user, nice, system, idle, iowait, irq, softirq, steal, guest, guest_nice (all in USER_HZ, typically centiseconds).

   Metrics to expose:
   - `sysmetrics_cpu_seconds_total{cpu="N", mode="user|nice|system|idle|iowait|irq|softirq|steal"}` — **counter**, convert from USER_HZ to seconds (divide by `sysconf(_SC_CLK_TCK)`, usually 100)
   - `sysmetrics_cpu_count` — **gauge**, number of logical CPUs

6. **Wire up axum server**
   - `GET /metrics` → calls all registered collectors, renders output, returns `text/plain; version=0.0.4; charset=utf-8`
   - `GET /health` → returns `200 OK` (useful for liveness probes)
   - `GET /` → returns a simple HTML page with a link to `/metrics`
   - Bind address and port configurable via CLI args (`--listen 0.0.0.0:9101`)
   - Add `tracing` middleware to log every request with method, path, status, and latency

### Acceptance criteria

- `curl localhost:9101/metrics` returns valid Prometheus text output with per-core CPU counters
- Output validates against `promtool check metrics` (install from Prometheus project)
- Server starts with a single log line showing bind address
- Graceful shutdown on SIGTERM/SIGINT

---

## Phase 2 — Memory, disk, and network collectors

**Goal:** Cover the four core metric domains that any ops engineer cares about.

### Tasks

7. **Memory collector** — parse `/proc/meminfo`
   Key lines to parse:
   ```
   MemTotal:       16384000 kB
   MemFree:         1234567 kB
   MemAvailable:    8765432 kB
   Buffers:          234567 kB
   Cached:          3456789 kB
   SwapTotal:       4194304 kB
   SwapFree:        4194000 kB
   ```
   Metrics to expose (all **gauge**, values in bytes — convert from kB):
   - `sysmetrics_memory_total_bytes`
   - `sysmetrics_memory_free_bytes`
   - `sysmetrics_memory_available_bytes`
   - `sysmetrics_memory_buffers_bytes`
   - `sysmetrics_memory_cached_bytes`
   - `sysmetrics_memory_swap_total_bytes`
   - `sysmetrics_memory_swap_free_bytes`
   - `sysmetrics_memory_used_bytes` — **computed**: total - free - buffers - cached

8. **Disk collector** — parse `/proc/diskstats`
   Format (selected columns):
   ```
   8  0 sda 12345 0 98765 4567 ...
   ```
   Fields (from kernel docs): major, minor, device, reads_completed, reads_merged, sectors_read, time_reading_ms, writes_completed, writes_merged, sectors_written, time_writing_ms, ios_in_progress, time_doing_ios_ms, weighted_time_ms

   Metrics to expose:
   - `sysmetrics_disk_reads_completed_total{device="sda"}` — counter
   - `sysmetrics_disk_writes_completed_total{device="sda"}` — counter
   - `sysmetrics_disk_read_bytes_total{device="sda"}` — counter (sectors × 512)
   - `sysmetrics_disk_written_bytes_total{device="sda"}` — counter (sectors × 512)
   - `sysmetrics_disk_io_time_seconds_total{device="sda"}` — counter (ms → seconds)
   - `sysmetrics_disk_io_in_progress{device="sda"}` — gauge

   Filtering: skip virtual devices (loop*, ram*, dm-*) by default, make the filter configurable.

9. **Network collector** — parse `/proc/net/dev`
   Format:
   ```
   Inter-|   Receive                                                |  Transmit
    face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
       lo: 1234567  12345    0    0    0     0          0         0  1234567  12345    0    0    0     0       0          0
     eth0: 9876543  98765    0    0    0     0          0         0  5432198  54321    0    0    0     0       0          0
   ```
   Metrics to expose:
   - `sysmetrics_network_receive_bytes_total{interface="eth0"}` — counter
   - `sysmetrics_network_transmit_bytes_total{interface="eth0"}` — counter
   - `sysmetrics_network_receive_packets_total{interface="eth0"}` — counter
   - `sysmetrics_network_transmit_packets_total{interface="eth0"}` — counter
   - `sysmetrics_network_receive_errors_total{interface="eth0"}` — counter
   - `sysmetrics_network_transmit_errors_total{interface="eth0"}` — counter
   - `sysmetrics_network_receive_drop_total{interface="eth0"}` — counter
   - `sysmetrics_network_transmit_drop_total{interface="eth0"}` — counter

   Filtering: skip `lo` by default, make configurable.

### Acceptance criteria

- All four collectors produce valid Prometheus output
- `promtool check metrics` passes on the full output
- Memory values match `free -b` output (within rounding)
- Disk and network counters match `/proc` values (verify manually)

---

## Phase 3 — Configuration, error handling, and operational polish

**Goal:** Make it production-grade, not a toy.

### Tasks

10. **TOML configuration file**
    ```toml
    [server]
    listen = "0.0.0.0:9101"
    metrics_path = "/metrics"

    [collectors]
    cpu = true
    memory = true
    disk = true
    network = true

    [collectors.disk]
    # regex pattern for devices to exclude
    exclude_pattern = "^(loop|ram|dm-)"

    [collectors.network]
    exclude_pattern = "^(lo|veth)"
    ```
    CLI args override config file values. Config file path via `--config /etc/sysmetrics/config.toml`. Sensible defaults if no config file exists.

11. **Proper error handling**
    - Define a `CollectorError` enum using `thiserror`:
      ```rust
      #[derive(Debug, thiserror::Error)]
      pub enum CollectorError {
          #[error("failed to read {path}: {source}")]
          FileRead { path: String, source: std::io::Error },

          #[error("failed to parse {field} in {path}: {raw}")]
          Parse { path: String, field: String, raw: String },
      }
      ```
    - If a single collector fails, the others still run. The failed collector logs an error and exposes `sysmetrics_collector_errors_total{collector="cpu"}` and `sysmetrics_collector_success{collector="cpu"}` (0 or 1) — **this is how node_exporter does it and it's the right pattern**.
    - No `.unwrap()` in any non-test code. Use `?` propagation or explicit error handling everywhere.
    - Every error should be actionable from the log line alone.

12. **Meta-metrics**
    - `sysmetrics_scrape_duration_seconds{collector="cpu"}` — gauge, how long each collector took
    - `sysmetrics_scrape_duration_seconds_total` — gauge, total scrape time
    - `sysmetrics_build_info{version="0.1.0", rustc="..."}` — gauge, always 1 (standard pattern)

13. **Structured logging with tracing**
    - Use `tracing` with `tracing-subscriber` for JSON-formatted logs
    - Log levels: startup info at `INFO`, each scrape at `DEBUG`, errors at `ERROR`
    - Include span context: collector name, scrape duration, client IP
    - CLI flag `--log-format text|json` (default: text for development, json for production)

14. **Graceful shutdown**
    - Handle SIGTERM and SIGINT via `tokio::signal`
    - Log shutdown initiation, drain in-flight requests, then exit
    - Exit code 0 on clean shutdown

---

## Phase 4 — Testing and CI

**Goal:** Demonstrate you know how to test systems code, not just application logic.

### Tasks

15. **Unit tests for parsers**
    - For each `/proc` parser, create test fixtures with representative content (include edge cases: single-core system, 128-core system, device names with hyphens, interfaces with colons)
    - Test the parsing logic against known expected values
    - Test malformed input produces clear errors, not panics
    - Example:
      ```rust
      #[cfg(test)]
      mod tests {
          use super::*;

          const PROC_STAT_FIXTURE: &str = "\
      cpu  74156 1260 22706 6316498 4539 0 456 0 0 0
      cpu0 18539 315 5676 1579124 1134 0 114 0 0 0
      cpu1 18540 315 5677 1579125 1135 0 114 0 0 0
      ";

          #[test]
          fn test_parse_cpu_stats() {
              let stats = parse_cpu_stats(PROC_STAT_FIXTURE).unwrap();
              assert_eq!(stats.len(), 2); // two physical CPUs
              assert_eq!(stats[0].user, 18539);
          }

          #[test]
          fn test_parse_cpu_stats_malformed() {
              let result = parse_cpu_stats("garbage data");
              assert!(result.is_err());
          }
      }
      ```

16. **Integration tests**
    - Spin up the server on a random port
    - Hit `/metrics` and verify:
      - HTTP 200 with correct content type
      - Output contains expected `# HELP` and `# TYPE` lines
      - All enabled collectors produced output
      - `promtool check metrics` validation (if available in CI)
    - Hit `/health` and verify 200
    - Test with individual collectors disabled via config

17. **Prometheus text format tests**
    - Test label escaping (backslashes, newlines, quotes in label values)
    - Test metric name sanitization
    - Test that counter and gauge types are correctly annotated
    - Golden file tests: compare rendered output against known-good snapshots

18. **GitHub Actions CI**
    ```yaml
    name: CI
    on: [push, pull_request]
    jobs:
      check:
        runs-on: ubuntu-latest
        steps:
          - uses: actions/checkout@v4
          - uses: dtolnay/rust-toolchain@stable
            with:
              components: rustfmt, clippy
          - run: cargo fmt --check
          - run: cargo clippy -- -D warnings
          - run: cargo test
          - run: cargo build --release
    ```
    - `cargo fmt --check` — no formatting violations
    - `cargo clippy -- -D warnings` — no linter warnings
    - `cargo test` — all tests pass
    - `cargo build --release` — clean release build

---

## Phase 5 — Documentation and presentation

**Goal:** Make it portfolio-ready. Oxide reviews your materials with extreme care.

### Tasks

19. **README.md** — should cover:
    - What it is and why it exists (one paragraph, not a novel)
    - Quick start: `cargo build --release && ./target/release/sysmetrics-rs`
    - Configuration reference (table format)
    - Example Prometheus scrape config snippet
    - Example Grafana dashboard screenshot (optional but impactful)
    - Architecture overview (the ASCII diagram from above or a Mermaid diagram)
    - Link to design decisions doc

20. **DESIGN.md** — this is where you show Oxide you think like an engineer:
    - Why read `/proc` directly instead of using a crate like `sysinfo`? (Answer: fewer dependencies, full control over parsing, understanding of the data source, same philosophy as Oxide's approach to building from first principles)
    - Why the collector trait pattern? (Answer: extensibility, testability, isolation of failures)
    - Why no background polling? (Answer: Prometheus pull model means freshness is guaranteed per-scrape, no stale data, simpler concurrency model)
    - Trade-offs made and what you'd do differently at scale
    - Performance characteristics: how fast is a scrape? What's the memory footprint?

21. **Benchmarks** (optional but strong signal)
    - Use `criterion` to benchmark individual collectors
    - Measure full scrape latency under load (`wrk` or `hey`)
    - Document results in DESIGN.md

---

## Recommended crate versions

```toml
[dependencies]
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
clap = { version = "4", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }
anyhow = "1"
thiserror = "2"
regex = "1"

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
reqwest = { version = "0.12", features = ["json"] }
tokio-test = "0.4"
```

---

## What this project proves to Oxide

This project is small enough to finish in 1–2 focused weekends, but it hits nearly every dimension they care about for the Operations Support Engineer role:

- **Rust proficiency** — async, traits, error handling, module organization, idiomatic patterns
- **Systems knowledge** — `/proc` filesystem, kernel metrics, understanding of what the numbers actually mean
- **Observability expertise** — Prometheus data model, metric types, exposition format, scrape mechanics
- **Operational mindset** — graceful shutdown, structured logging, health endpoints, failure isolation
- **Engineering rigor** — tests, CI, clippy clean, documentation, design rationale
- **First-principles thinking** — parsing `/proc` yourself instead of reaching for a crate mirrors Oxide's build-it-right philosophy

Ship it clean, write good commit messages, and reference it in your application materials as a concrete demonstration of your Rust + infrastructure chops.
