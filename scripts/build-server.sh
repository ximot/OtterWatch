#!/bin/bash
# Build OtterWatch Server Docker Image

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECTS_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"
SERVER_PATH="$PROJECTS_ROOT/otterwatch-server"

REGISTRY="${DOCKER_REGISTRY:-localhost:5000}"
VERSION="${VERSION:-latest}"
BUILD_TOOL="${BUILD_TOOL:-docker}"

echo "Building otterwatch-server:${VERSION}"
cd "$SERVER_PATH"

$BUILD_TOOL build -t "${REGISTRY}/otterwatch-server:${VERSION}" .
$BUILD_TOOL tag "${REGISTRY}/otterwatch-server:${VERSION}" "${REGISTRY}/otterwatch-server:latest"

echo "✓ Built: ${REGISTRY}/otterwatch-server:${VERSION}"
