# sysmetrics-rs

A lightweight Prometheus exporter for Linux system metrics. Reads directly from `/proc` and `/sys` — no C bindings or wrapper crates. Built with async Rust (Tokio + Axum).

Exposes CPU, memory, disk I/O, and network metrics on port **9101** in Prometheus exposition format.

## Metrics

| Collector | Source | Example metrics |
|-----------|--------|-----------------|
| CPU | `/proc/stat` | `sysmetrics_cpu_seconds_total{cpu="0", mode="user"}`, `sysmetrics_cpu_count` |
| Memory | `/proc/meminfo` | `sysmetrics_memory_total_bytes`, `sysmetrics_memory_available_bytes`, `sysmetrics_memory_used_bytes` |
| Disk | `/proc/diskstats` | `sysmetrics_disk_read_bytes_total{device="sda"}`, `sysmetrics_disk_writes_completed_total` |
| Network | `/proc/net/dev` | `sysmetrics_network_receive_bytes_total{interface="eth0"}`, `sysmetrics_network_transmit_bytes_total` |

The exporter also generates scrape meta-metrics (`sysmetrics_scrape_duration_seconds`, `sysmetrics_collector_success`, `sysmetrics_build_info`).

## Installation

### Pre-built binaries

Download from the [GitHub Releases](https://github.com/Horkyze/rust-system-prometheus-exporter/releases) page. Binaries are available for:

| Target | Description |
|--------|-------------|
| `x86_64-unknown-linux-gnu` | Linux x86_64 (glibc) |
| `x86_64-unknown-linux-musl` | Linux x86_64 (static, musl) |
| `aarch64-unknown-linux-gnu` | Linux ARM64 (glibc) |
| `aarch64-unknown-linux-musl` | Linux ARM64 (static, musl) |

```bash
# Download and extract (example for x86_64 glibc)
curl -LO https://github.com/Horkyze/rust-system-prometheus-exporter/releases/latest/download/sysmetrics-rs-v0.1.1-x86_64-unknown-linux-gnu.tar.gz
tar xzf sysmetrics-rs-v0.1.1-x86_64-unknown-linux-gnu.tar.gz
sudo mv sysmetrics-rs-v0.1.1-x86_64-unknown-linux-gnu/sysmetrics-rs /usr/local/bin/
```

### Debian/Ubuntu (.deb)

`.deb` packages are published for `amd64` and `arm64`:

```bash
curl -LO https://github.com/Horkyze/rust-system-prometheus-exporter/releases/latest/download/sysmetrics-rs_0.1.1_amd64.deb
sudo dpkg -i sysmetrics-rs_0.1.1_amd64.deb
```

The package installs:

- Binary at `/usr/bin/sysmetrics-rs`
- Config at `/etc/sysmetrics-rs/config.toml`
- Systemd service `sysmetrics-rs.service`

Start the service:

```bash
sudo systemctl start sysmetrics-rs
sudo systemctl enable sysmetrics-rs
```

### Docker

```bash
docker run -d \
  --name sysmetrics \
  -p 9101:9101 \
  -v /proc:/host/proc:ro \
  -v /sys:/host/sys:ro \
  ghcr.io/horkyze/rust-system-prometheus-exporter:latest
```

### Build from source

Requires Rust 1.70+ and a Linux system.

```bash
git clone https://github.com/Horkyze/rust-system-prometheus-exporter.git
cd rust-system-prometheus-exporter
cargo build --release
sudo cp target/release/sysmetrics-rs /usr/local/bin/
```

## Usage

```bash
# Run with defaults (listens on 0.0.0.0:9101)
sysmetrics-rs

# Bind to a specific address
sysmetrics-rs --listen 127.0.0.1:9101

# Use a config file
sysmetrics-rs --config /etc/sysmetrics-rs/config.toml

# JSON log output
sysmetrics-rs --log-format json
```

### Endpoints

| Path | Description |
|------|-------------|
| `/metrics` | Prometheus metrics |
| `/health` | Health check |
| `/` | Basic info page |

### Verify it works

```bash
curl http://localhost:9101/metrics
```

## Configuration

Configuration is via TOML file, CLI flags, or both. CLI flags override values from the config file.

### CLI flags

| Flag | Default | Description |
|------|---------|-------------|
| `--listen <ADDR>` | `0.0.0.0:9101` | Address and port to listen on |
| `--config <PATH>` | — | Path to TOML configuration file |
| `--log-format <FORMAT>` | `text` | Log format: `text` or `json` |

### Config file

```toml
[server]
listen = "0.0.0.0:9101"
metrics_path = "/metrics"
log_format = "text"

[collectors]
cpu = true
memory = true
disk = true
network = true

[collectors.disk_config]
# Regex pattern — matching devices are excluded
exclude_pattern = "^(loop|ram|dm-)"

[collectors.network_config]
# Regex pattern — matching interfaces are excluded
exclude_pattern = "^(lo|veth)"
```

Set any collector to `false` to disable it. Adjust the `exclude_pattern` regex to control which disk devices or network interfaces are reported.

## Prometheus scrape config

Add to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: "sysmetrics"
    static_configs:
      - targets: ["<host>:9101"]
```

## Running as a systemd service

If you installed from a `.deb` package, the service is already set up. For manual installations, create `/etc/systemd/system/sysmetrics-rs.service`:

```ini
[Unit]
Description=sysmetrics-rs - Linux system metrics Prometheus exporter
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=sysmetrics
Group=sysmetrics
ExecStart=/usr/local/bin/sysmetrics-rs --config /etc/sysmetrics-rs/config.toml
Restart=on-failure
RestartSec=5s
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadOnlyPaths=/proc /sys

[Install]
WantedBy=multi-user.target
```

Then:

```bash
sudo useradd --system --no-create-home --shell /usr/sbin/nologin sysmetrics
sudo mkdir -p /etc/sysmetrics-rs
sudo cp config.toml /etc/sysmetrics-rs/config.toml
sudo systemctl daemon-reload
sudo systemctl enable --now sysmetrics-rs
```

## Requirements

- **Linux only** — reads from `/proc` and `/sys`, which are not available on macOS or Windows.

## License

MIT
