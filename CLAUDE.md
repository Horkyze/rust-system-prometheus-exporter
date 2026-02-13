# CLAUDE.md

## Project Overview

**sysmetrics-rs** is a lightweight, async Prometheus exporter written in Rust that reads Linux system metrics directly from `/proc` and `/sys` filesystems. It exposes CPU, memory, disk I/O, and network metrics in Prometheus exposition format via an HTTP server on port 9101.

Key design principle: no C bindings or high-level system info crates. All metrics are parsed from raw `/proc` files using first-principles string parsing.

## Build & Development Commands

```bash
# Build (debug)
cargo build

# Build (release)
cargo build --release

# Run all tests (unit + integration)
cargo test

# Run only unit tests (faster, no server spawn)
cargo test --lib

# Check formatting
cargo fmt --check

# Auto-format
cargo fmt

# Lint with clippy (CI uses -Dwarnings)
cargo clippy --all-targets -- -D warnings

# Run the server locally
cargo run -- --listen 127.0.0.1:9101

# Run with a config file
cargo run -- --config config.toml

# Run with JSON logging
cargo run -- --log-format json
```

CI enforces `RUSTFLAGS="-Dwarnings"` — all warnings are treated as errors.

## Project Structure

```
src/
  main.rs              # Entry point: CLI parsing, logging setup, collector registration, server startup
  config.rs            # TOML config loading, CLI arg definitions (clap derive), defaults
  error.rs             # CollectorError enum (FileRead, Parse) via thiserror
  server.rs            # Axum HTTP routes: /, /health, /metrics; meta-metrics assembly
  collector/
    mod.rs             # Collector trait, Registry, MetricType/Metric/MetricSample types, Prometheus renderer
    cpu.rs             # CpuCollector — parses /proc/stat for per-core CPU time
    memory.rs          # MemoryCollector — parses /proc/meminfo for memory/swap stats
    disk.rs            # DiskCollector — parses /proc/diskstats with regex device filtering
    network.rs         # NetworkCollector — parses /proc/net/dev with regex interface filtering
tests/
  integration_test.rs  # Spawns actual server, tests HTTP endpoints end-to-end
  fixtures/
    proc_stat_128.txt  # Test fixture simulating 128-core /proc/stat output
```

## Architecture

### Collector Pattern

All metric collectors implement the `Collector` trait (`src/collector/mod.rs`):

```rust
pub trait Collector: Send + Sync {
    fn name(&self) -> &'static str;
    fn collect(&self) -> Result<Vec<Metric>, CollectorError>;
}
```

The `Registry` holds `Vec<Box<dyn Collector>>` and calls each on every `/metrics` scrape. Collectors are stateless (no background polling) — metrics are read on-demand per request.

Failure isolation: if one collector fails, others still return their metrics. The server reports per-collector success/failure via meta-metrics.

### Adding a New Collector

1. Create `src/collector/<name>.rs` implementing the `Collector` trait
2. Add `pub mod <name>;` to `src/collector/mod.rs`
3. Register in `src/main.rs` inside the collector registration block
4. Add enable/disable config in `src/config.rs` under `CollectorsConfig`
5. Add unit tests inline with `#[cfg(test)]` module

### Request Flow

`GET /metrics` → `server::metrics_handler` → `Registry::collect_all()` → each collector reads `/proc/*` → results + meta-metrics rendered via `render_metrics()` → returned as `text/plain; version=0.0.4; charset=utf-8`

### Meta-Metrics

The server generates these automatically on every scrape:
- `sysmetrics_scrape_duration_seconds{collector="X"}` (gauge)
- `sysmetrics_scrape_duration_seconds_total` (gauge)
- `sysmetrics_collector_success{collector="X"}` (gauge, 1 or 0)
- `sysmetrics_collector_errors_total{collector="X"}` (counter)
- `sysmetrics_build_info{version, rustc}` (gauge, always 1)

## Code Conventions

### Naming

- **Metric names**: `sysmetrics_` prefix, snake_case (e.g., `sysmetrics_cpu_seconds_total`)
- **Metric label keys**: snake_case (e.g., `cpu`, `mode`, `device`, `interface`)
- **Collector names**: lowercase strings returned by `name()` (e.g., `"cpu"`, `"memory"`)
- **Structs**: PascalCase (e.g., `CpuCollector`, `DiskStats`, `NetStats`)
- **Constants**: UPPER_SNAKE_CASE (e.g., `PROC_STAT_PATH`, `USER_HZ`, `SECTOR_SIZE`)
- **Parsing functions**: standalone free functions, not methods, for testability

### Error Handling

- `CollectorError` (thiserror) for collector-level errors with context (path, field, raw value)
- `anyhow::Result` at the application top level (`main`)
- No `.unwrap()` in production code — only in tests and `expect()` for programmer errors
- `?` operator for propagation throughout

### Testing

- Unit tests inline via `#[cfg(test)]` in each module
- Test parsing with hardcoded string constants mimicking `/proc` file contents
- Integration tests spawn a real server process and make HTTP requests
- Fixture files in `tests/fixtures/` for large test inputs
- ~37 unit tests + integration test suite

### Dependencies

Production: axum, tokio (full), serde, toml, clap, tracing, tracing-subscriber, anyhow, thiserror, regex, tower-http
Dev only: reqwest, tokio-test, tower

### Formatting and Linting

- Default `rustfmt` settings (no `rustfmt.toml`)
- Default `clippy` settings (no `clippy.toml`)
- CI runs `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`

## Configuration

The server accepts configuration via TOML file and/or CLI flags. CLI flags override TOML values.

### CLI Flags

- `--listen <ADDR>` (default: `0.0.0.0:9101`)
- `--config <PATH>` (optional TOML config file)
- `--log-format <text|json>` (default: `text`)

### TOML Config Sections

- `[server]`: listen, metrics_path, log_format
- `[collectors]`: cpu, memory, disk, network (bool enable/disable)
- `[collectors.disk]`: exclude_pattern (regex, default: `^(loop|ram|dm-)`)
- `[collectors.network]`: exclude_pattern (regex, default: `^(lo|veth)`)

## CI/CD

- **CI** (`.github/workflows/ci.yml`): fmt check, clippy, tests, release build — runs on push to main/master and PRs
- **Release** (`.github/workflows/release.yml`): triggered by `v*.*.*` tags — builds for x86_64/aarch64 (gnu+musl), creates GitHub release with checksums, builds Docker image to ghcr.io

## Docker

Multi-stage build (`Dockerfile`): compiles with `rust:1-slim`, runs on `debian:bookworm-slim`. Exposes port 9101. Default entrypoint: `sysmetrics-rs --listen 0.0.0.0:9101`.

## Important Notes

- This project only runs on Linux — it reads from `/proc` which doesn't exist on macOS/Windows
- Integration tests check for `/proc/stat` and `/proc/meminfo` existence and skip metric assertions if absent
- Disk and network collectors use compiled regex for device/interface filtering — regex patterns are compiled once at collector construction
- All values from `/proc` use saturating arithmetic where underflow is possible (e.g., memory used calculation)
- CPU times are converted from USER_HZ (centiseconds) to seconds; disk sectors are 512 bytes each
