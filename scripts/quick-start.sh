#!/bin/bash
# OtterWatch Quick Start
# Builds images and starts the complete stack

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=========================================="
echo "OtterWatch Quick Start"
echo "=========================================="
echo ""

# Check if .env exists
if [ ! -f "$SCRIPT_DIR/.env" ]; then
    echo "⚠️  No .env file found. Creating from .env.example..."
    cp "$SCRIPT_DIR/.env.example" "$SCRIPT_DIR/.env"
    echo ""
    echo "⚠️  WARNING: Please edit .env and change default passwords/API keys!"
    echo "   Edit: $SCRIPT_DIR/.env"
    echo ""
    read -p "Press Enter to continue after editing .env, or Ctrl+C to abort..."
fi

# Load .env
source "$SCRIPT_DIR/.env"

echo "Step 1/3: Building Docker images..."
echo ""
"$SCRIPT_DIR/build-all-images.sh"

echo ""
echo "Step 2/3: Starting OtterWatch stack..."
echo ""
cd "$SCRIPT_DIR"
docker-compose -f docker-compose-full.yml up -d

echo ""
echo "Step 3/3: Waiting for services to become healthy..."
echo ""
sleep 5

# Check service health
echo "Checking PostgreSQL..."
until docker-compose -f docker-compose-full.yml exec -T db pg_isready -U otterwatch -d otterwatch > /dev/null 2>&1; do
    echo "  Waiting for PostgreSQL..."
    sleep 2
done
echo "  ✓ PostgreSQL ready"

echo "Checking MQTT broker..."
until curl -sf http://localhost:${MQTT_API_PORT:-8085}/health > /dev/null 2>&1; do
    echo "  Waiting for MQTT broker..."
    sleep 2
done
echo "  ✓ MQTT broker ready"

echo "Checking Server..."
until curl -sf http://localhost:${SERVER_PORT:-8080}/health > /dev/null 2>&1; do
    echo "  Waiting for Server..."
    sleep 2
done
echo "  ✓ Server ready"

echo ""
echo "=========================================="
echo "✓ OtterWatch is ready!"
echo "=========================================="
echo ""
echo "Access points:"
echo "  Dashboard:    http://localhost:${SERVER_PORT:-8080}"
echo "  Server API:   http://localhost:${SERVER_PORT:-8080}/api"
echo "  MQTT Broker:  tcp://localhost:${MQTT_PORT:-1883}"
echo "  MQTT API:     http://localhost:${MQTT_API_PORT:-8085}"
echo "  Prometheus:   http://localhost:${MQTT_METRICS_PORT:-9090}/metrics"
echo ""
echo "Useful commands:"
echo "  View logs:    docker-compose -f $SCRIPT_DIR/docker-compose-full.yml logs -f"
echo "  Stop stack:   docker-compose -f $SCRIPT_DIR/docker-compose-full.yml down"
echo "  Restart:      docker-compose -f $SCRIPT_DIR/docker-compose-full.yml restart"
echo ""
