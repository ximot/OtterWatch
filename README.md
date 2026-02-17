![OtterWatch](media/logo.png)

# OtterWatch Agent

> Lightweight Linux system monitoring agent written in Rust

**Version:** 0.3.6  
**License:** MIT  
**Author:** Tomasz Wyderka

## 📋 Overview

OtterWatch Agent is a high-performance monitoring agent that collects system metrics from the Linux kernel's `/proc` filesystem and transmits them to a central server via MQTT. Part of the complete OtterWatch monitoring ecosystem.

### Key Features

✅ **Comprehensive Metrics Collection**
- CPU usage with I/O wait breakdown
- RAM and Swap memory statistics
- Disk I/O operations (read, write, wait times)
- Network interface statistics
- Pressure Stall Information (PSI)
- Top N processes with resource usage

✅ **Flexible Data Transport**
- Push to MQTT broker (Protocol Buffers)
- Local JSONL storage for historical data
- Local REST API for real-time queries
- Offline queue with automatic retry

✅ **Remote Management**
- Self-update system with SHA256 verification
- Remote command execution (restart, reload-config, etc.)
- Bootstrap auto-discovery of MQTT broker
- Agent grouping for fleet organization

✅ **Developer-Friendly**
- Terminal UI with `--gui` flag for debugging
- Minimal resource consumption
- Written in Rust for safety and performance

---

## 🚀 Quick Start

### Installation (Ubuntu/Debian)

```bash
# Download the latest release
wget https://github.com/ximot/otterwatch/releases/latest/download/otterwatch-0.3.6.tar.gz
tar -xzf otterwatch-0.3.6.tar.gz
cd otterwatch-0.3.6

# Edit configuration
cp settings.toml.example settings.toml
nano settings.toml

# Install as systemd service
sudo ./install.sh

# Start the agent
sudo systemctl start otterwatch
sudo systemctl status otterwatch
```

### Building from Source

```bash
# Prerequisites
sudo apt-get install build-essential pkg-config libssl-dev protobuf-compiler

# Clone and build
git clone https://github.com/ximot/otterwatch.git
cd otterwatch
cargo build --release

# Run
./target/release/otterwatch

# Or with console UI
./target/release/otterwatch --gui
```

### Docker Deployment

See the [Docker deployment guide](scripts/README-DOCKER.md) for running the complete OtterWatch stack.

---

## ⚙️ Configuration

Configuration file: `settings.toml`

```toml
# Collection settings
interval_secs = 1                    # Metrics collection interval
process_list_interval_secs = 10      # Process list collection interval
process_top_n = 25                   # Number of top processes to track

# Local HTTP API
listen_addr = "127.0.0.1:8080"       # Local API address
cors_allowed_origins = "*"           # CORS origins

# Local storage
db_file_name = "system_stats.db"     # Local database directory
db_save = true                       # Enable local storage
db_history_days = 31                 # Days to keep historical data
exclude_interfaces = "lo,wlan0"      # Network interfaces to exclude

# MQTT Push Configuration
mqtt_enabled = true                                    # Enable MQTT
mqtt_broker_addr = "tcp://localhost:1883"              # MQTT broker
mqtt_broker_addrs = ["tcp://broker1:1883", "..."]      # Failover brokers
mqtt_api_key = "your-secret-key"                       # API key for auth
mqtt_topic_prefix = "otterwatch/metrics"               # Topic prefix
mqtt_queue_path = "mqtt_queue"                         # Offline queue dir
mqtt_queue_max_size_mb = 100                           # Max queue size
mqtt_keepalive_secs = 30                               # Keepalive interval
mqtt_retry_interval_secs = 5                           # Reconnect interval

# Bootstrap - auto-discover broker from central server
mqtt_bootstrap_url = "http://server:8080/api/cluster/bootstrap"
mqtt_bootstrap_timeout_secs = 10

# Agent grouping (for fleet organization)
agent_group = "web-servers"
```

**Environment Variables:** Prefix with `APP_` to override, e.g., `APP_MQTT_API_KEY=secret`

---

## 🔌 API Endpoints

Local REST API (default: `http://127.0.0.1:8080`)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/system-stats` | GET | Current CPU, memory, swap usage |
| `/system-info` | GET | OS information (hostname, kernel, etc.) |

Example:
```bash
curl http://127.0.0.1:8080/system-stats | jq
```

---

## 🎮 Remote Commands

Agent can receive commands via MQTT from the central server:

| Command | Description |
|---------|-------------|
| `ping` | Health check with RTT measurement |
| `reload-config` | Reload settings.toml without restart |
| `restart` | Restart agent (via systemd) |
| `reconnect` | Re-run bootstrap and reconnect to new broker |
| `set-group` | Update agent group |
| `update` | Download and apply new agent version |
| `get-config` | Return current configuration |
| `set-config` | Update single config parameter |
| `sync-config` | Add missing config keys with defaults |
| `get-config-schema` | Return full config schema |
| `get-swap-processes` | List processes using swap memory |

---

## 🏗️ Architecture

### Main Components

```
/proc/* → Metrics Collection → Local Storage (JSONL)
                             ↓
                          MQTT Client → Broker → Central Server
                             ↑
                      Command Handler ← MQTT Subscribe
```

### Source Modules

- **Metrics:** `cpu.rs`, `memory.rs`, `disk_io.rs`, `network.rs`, `pressure.rs`, `process.rs`
- **Communication:** `mqtt_client.rs`, `message_queue.rs`, `bootstrap.rs`
- **Storage:** `storage.rs`, `app_config.rs`
- **UI:** `console_ui.rs`, `osinfo.rs`
- **Core:** `main.rs`, `agent_id.rs`, `self_update.rs`

---

## 🌐 OtterWatch Ecosystem

The complete monitoring solution consists of 4 components:

1. **otterwatch** (this project) - Agent collecting metrics on monitored machines
2. **otterwatch-mqtt** - Custom MQTT broker optimized for OtterWatch
3. **otterwatch-server** - Central server with PostgreSQL/TimescaleDB storage
4. **otterwatch-ai** - AI analysis agent using Ollama for fleet intelligence

### Data Flow

```
┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
│ Agent(s) │────→│   MQTT   │────→│  Server  │────→│   AI     │
│          │     │  Broker  │     │          │     │ Agent    │
└──────────┘     └──────────┘     └────┬─────┘     └──────────┘
                                        │                 ↑
                                        ↓                 │
                                   PostgreSQL/        Read-only
                                   TimescaleDB          access
```

---

## 🔧 Development

### Build Commands

```bash
# Development build
cargo build

# Release build
cargo build --release

# Run with console UI (debug)
cargo run -- --gui

# Run tests
cargo test

# Code formatting
cargo fmt

# Linting
cargo clippy

# Build release package
./scripts/build-release.sh
```

### Release Package Contents

```
dist/
├── otterwatch              # Release binary
├── otterwatch-X.Y.Z        # Versioned binary (for server updates)
├── otterwatch-wrapper.sh   # Wrapper script for self-updates
├── otterwatch.service      # Systemd service file
├── settings.toml.example   # Example configuration
├── install.sh              # Installation script
├── uninstall.sh            # Uninstallation script
└── checksums.sha256        # SHA256 checksums
```

---

## 🐳 Docker & Container Deployment

Complete Docker deployment scripts are available in `scripts/`:

```bash
# Build all images (MQTT + Server + DB)
cd scripts/
./build-all-images.sh

# Quick start entire stack
./quick-start.sh

# Or manually with docker-compose
docker-compose -f docker-compose-full.yml up -d
```

See [scripts/README-DOCKER.md](scripts/README-DOCKER.md) for full documentation.

---

## 🔐 Security

### Agent ID Persistence

Agent ID priority (highest to lowest):
1. `/etc/otterwatch/agent_id` - System-wide, survives reinstalls
2. `{db_file_name}/agent_id` - Local, for backwards compatibility
3. Derived from `/etc/machine-id` - Deterministic per machine
4. Random UUID - Fallback

### Root Detection

Agent reports whether it's running as root via `is_root` field:
- **Root (UID 0):** Full access to all process information
- **Non-root:** Limited visibility, own processes only

---

## 📊 Metrics Collected

### System Metrics (every 1s by default)
- CPU: user, system, idle, iowait, steal
- Memory: total, used, available, swap
- Disk I/O: reads, writes, wait time, I/O time
- Network: rx_bytes, tx_bytes, rx_packets, tx_packets
- PSI: CPU, memory, I/O pressure stalls

### Process Metrics (every 10s by default)
- Top N processes by CPU and memory
- Per-process: PID, name, user, CPU%, memory%, swap

---

## 🛠️ Troubleshooting

### Agent not connecting to MQTT

```bash
# Check configuration
cat settings.toml | grep mqtt

# Test broker connectivity
nc -zv broker-host 1883

# Check agent logs
journalctl -u otterwatch -f

# Run with debug logging
RUST_LOG=debug ./otterwatch
```

### Missing metrics

Agent requires **Linux** and access to `/proc` filesystem:
```bash
# Verify proc access
ls -la /proc/stat /proc/meminfo /proc/diskstats

# Check permissions (some metrics need root)
sudo ./otterwatch
```

### Self-update failed

```bash
# Check wrapper script
systemctl cat otterwatch | grep ExecStart

# Manual rollback
sudo mv /usr/bin/otterwatch.backup /usr/bin/otterwatch
sudo systemctl restart otterwatch
```

---

## 📝 License

MIT License - see [LICENSE](LICENSE) file

---

## 🤝 Contributing

Contributions welcome! Please:
1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests: `cargo test`
5. Format code: `cargo fmt`
6. Submit a pull request

---

## 🔗 Links

- **GitHub:** https://github.com/ximot/otterwatch
- **Documentation:** See [README.md](README.md) for technical details
- **Server:** https://github.com/ximot/otterwatch-server
- **MQTT Broker:** https://github.com/ximot/otterwatch-mqtt
- **AI Agent:** https://github.com/ximot/otterwatch-ai

---

**Made with ❤️ in Rust**
