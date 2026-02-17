#!/bin/bash
#
# OtterWatch Agent Build Script
# Builds release binary and prepares deployment package
#
# Usage:
#   ./scripts/build-release.sh [output-dir]
#
# Output structure:
#   output-dir/
#   ├── otterwatch                    # Release binary
#   ├── otterwatch-X.Y.Z              # Versioned binary (for server updates/)
#   ├── otterwatch-wrapper.sh         # Wrapper script
#   ├── otterwatch.service            # Systemd service file
#   ├── settings.toml.example         # Example configuration
#   ├── install.sh                    # Installation script
#   └── checksums.sha256              # SHA256 checksums
#

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Get script directory and project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Output directory (default: dist/)
OUTPUT_DIR="${1:-$PROJECT_ROOT/dist}"

# Get version from Cargo.toml
VERSION=$(grep '^version' "$PROJECT_ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}  OtterWatch Agent Build Script${NC}"
echo -e "${GREEN}  Version: $VERSION${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""

# Check we're in the right directory
if [[ ! -f "$PROJECT_ROOT/Cargo.toml" ]]; then
    echo -e "${RED}Error: Cargo.toml not found. Run from project root or scripts/ directory.${NC}"
    exit 1
fi

# Create output directory
echo -e "${YELLOW}Creating output directory: $OUTPUT_DIR${NC}"
rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

# Build release binary
echo -e "${YELLOW}Building release binary...${NC}"
cd "$PROJECT_ROOT"
cargo build --release

if [[ ! -f "$PROJECT_ROOT/target/release/otterwatch" ]]; then
    echo -e "${RED}Error: Build failed - binary not found${NC}"
    exit 1
fi

echo -e "${GREEN}Build successful!${NC}"

# Copy binary
echo -e "${YELLOW}Copying files...${NC}"
cp "$PROJECT_ROOT/target/release/otterwatch" "$OUTPUT_DIR/otterwatch"
cp "$PROJECT_ROOT/target/release/otterwatch" "$OUTPUT_DIR/otterwatch-$VERSION"

# Copy scripts
cp "$SCRIPT_DIR/otterwatch-wrapper.sh" "$OUTPUT_DIR/"
cp "$SCRIPT_DIR/otterwatch.service" "$OUTPUT_DIR/"

# Create example settings
cat > "$OUTPUT_DIR/settings.toml.example" << 'EOF'
# OtterWatch Agent Configuration
# Copy this file to settings.toml and adjust values

# =============================================================================
# Collection Settings
# =============================================================================

# How often to collect system metrics (seconds)
interval_secs = 1

# How often to collect process list (seconds)
process_list_interval_secs = 10

# Number of top processes to track (by CPU usage)
process_top_n = 25

# =============================================================================
# Local HTTP API
# =============================================================================

# Address to bind local API (use 127.0.0.1 for local-only access)
listen_addr = "127.0.0.1:8080"

# CORS allowed origins (comma-separated, or "*" for all)
cors_allowed_origins = "*"

# =============================================================================
# Local Storage
# =============================================================================

# Directory for local data storage
db_file_name = "/var/lib/otterwatch"

# Enable local JSONL storage
db_save = false

# Days to keep local history
db_history_days = 7

# Network interfaces to exclude (comma-separated)
exclude_interfaces = "lo"

# =============================================================================
# MQTT Push Configuration (to central server)
# =============================================================================

# Enable MQTT push to central server
mqtt_enabled = true

# MQTT broker address
mqtt_broker_addr = "tcp://your-server:1883"

# API key for authentication (keep secret!)
# Can also be set via APP_MQTT_API_KEY environment variable
mqtt_api_key = "your-secret-api-key"

# MQTT topic prefix
mqtt_topic_prefix = "otterwatch/metrics"

# Offline queue settings
mqtt_queue_path = "mqtt_queue"
mqtt_queue_max_size_mb = 100

# Connection settings
mqtt_keepalive_secs = 30
mqtt_retry_interval_secs = 5

# =============================================================================
# Agent Identity
# =============================================================================

# Agent group (for organizing fleet in dashboard)
agent_group = ""
EOF

# Create installation script
cat > "$OUTPUT_DIR/install.sh" << 'INSTALL_EOF'
#!/bin/bash
#
# OtterWatch Agent Installation Script
#
# Usage:
#   sudo ./install.sh
#
# This script will:
#   1. Install binary to /usr/bin/
#   2. Install wrapper script to /usr/bin/
#   3. Create config directory /etc/otterwatch/
#   4. Create data directory /var/lib/otterwatch/
#   5. Install systemd service
#   6. Enable and start the service
#

set -e

# Check root
if [[ $EUID -ne 0 ]]; then
    echo "This script must be run as root (use sudo)"
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Installing OtterWatch Agent..."

# Create directories
echo "Creating directories..."
mkdir -p /etc/otterwatch
mkdir -p /var/lib/otterwatch
mkdir -p /var/log

# Install binaries
echo "Installing binaries..."
cp "$SCRIPT_DIR/otterwatch" /usr/bin/otterwatch
cp "$SCRIPT_DIR/otterwatch-wrapper.sh" /usr/bin/otterwatch-wrapper.sh
chmod +x /usr/bin/otterwatch
chmod +x /usr/bin/otterwatch-wrapper.sh

# Install config if not exists
if [[ ! -f /etc/otterwatch/settings.toml ]]; then
    echo "Installing example configuration..."
    cp "$SCRIPT_DIR/settings.toml.example" /etc/otterwatch/settings.toml
    chmod 600 /etc/otterwatch/settings.toml
    echo ""
    echo "IMPORTANT: Edit /etc/otterwatch/settings.toml before starting!"
    echo "  - Set mqtt_broker_addr to your server address"
    echo "  - Set mqtt_api_key to your API key"
    echo ""
else
    echo "Configuration file already exists, skipping..."
fi

# Install systemd service
echo "Installing systemd service..."
cp "$SCRIPT_DIR/otterwatch.service" /etc/systemd/system/otterwatch.service
systemctl daemon-reload

echo ""
echo "Installation complete!"
echo ""
echo "Next steps:"
echo "  1. Edit configuration: sudo nano /etc/otterwatch/settings.toml"
echo "  2. Enable service: sudo systemctl enable otterwatch"
echo "  3. Start service: sudo systemctl start otterwatch"
echo "  4. Check status: sudo systemctl status otterwatch"
echo "  5. View logs: sudo journalctl -u otterwatch -f"
echo ""
INSTALL_EOF

chmod +x "$OUTPUT_DIR/install.sh"

# Create uninstall script
cat > "$OUTPUT_DIR/uninstall.sh" << 'UNINSTALL_EOF'
#!/bin/bash
#
# OtterWatch Agent Uninstallation Script
#

set -e

if [[ $EUID -ne 0 ]]; then
    echo "This script must be run as root (use sudo)"
    exit 1
fi

echo "Uninstalling OtterWatch Agent..."

# Stop and disable service
echo "Stopping service..."
systemctl stop otterwatch 2>/dev/null || true
systemctl disable otterwatch 2>/dev/null || true

# Remove files
echo "Removing files..."
rm -f /usr/bin/otterwatch
rm -f /usr/bin/otterwatch-wrapper.sh
rm -f /etc/systemd/system/otterwatch.service
systemctl daemon-reload

echo ""
echo "Uninstallation complete!"
echo ""
echo "Note: Configuration and data directories were NOT removed:"
echo "  - /etc/otterwatch/"
echo "  - /var/lib/otterwatch/"
echo ""
echo "To remove them: sudo rm -rf /etc/otterwatch /var/lib/otterwatch"
echo ""
UNINSTALL_EOF

chmod +x "$OUTPUT_DIR/uninstall.sh"

# Generate checksums
echo -e "${YELLOW}Generating checksums...${NC}"
cd "$OUTPUT_DIR"
sha256sum otterwatch otterwatch-$VERSION > checksums.sha256

# Print summary
BINARY_SIZE=$(du -h "$OUTPUT_DIR/otterwatch" | cut -f1)
CHECKSUM=$(sha256sum "$OUTPUT_DIR/otterwatch" | cut -d' ' -f1)

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}  Build Complete!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo -e "Version:     ${YELLOW}$VERSION${NC}"
echo -e "Binary size: ${YELLOW}$BINARY_SIZE${NC}"
echo -e "SHA256:      ${YELLOW}$CHECKSUM${NC}"
echo ""
echo -e "Output directory: ${YELLOW}$OUTPUT_DIR${NC}"
echo ""
echo "Contents:"
ls -la "$OUTPUT_DIR"
echo ""
echo -e "${GREEN}To install on target system:${NC}"
echo "  1. Copy the dist/ directory to target machine"
echo "  2. Run: sudo ./install.sh"
echo ""
echo -e "${GREEN}To upload to server for remote updates:${NC}"
echo "  Copy otterwatch-$VERSION to server's updates/ directory"
echo ""
