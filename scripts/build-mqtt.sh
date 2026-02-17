#!/bin/bash
# Build OtterWatch MQTT Broker Docker Image

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECTS_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"
MQTT_PATH="$PROJECTS_ROOT/otterwatch-mqtt"

REGISTRY="${DOCKER_REGISTRY:-localhost:5000}"
VERSION="${VERSION:-latest}"
BUILD_TOOL="${BUILD_TOOL:-docker}"

echo "Building otterwatch-mqtt:${VERSION}"
cd "$MQTT_PATH"

$BUILD_TOOL build -t "${REGISTRY}/otterwatch-mqtt:${VERSION}" .
$BUILD_TOOL tag "${REGISTRY}/otterwatch-mqtt:${VERSION}" "${REGISTRY}/otterwatch-mqtt:latest"

echo "✓ Built: ${REGISTRY}/otterwatch-mqtt:${VERSION}"
