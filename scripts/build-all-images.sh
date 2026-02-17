#!/bin/bash
# OtterWatch - Build All Docker Images
# This script builds Docker images for all components of the OtterWatch ecosystem

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
REGISTRY="${DOCKER_REGISTRY:-localhost:5000}"
VERSION="${VERSION:-latest}"
BUILD_TOOL="${BUILD_TOOL:-docker}" # Can be 'docker' or 'podman'

# Project paths
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECTS_ROOT="$(dirname "$(dirname "$SCRIPT_DIR")")"
SERVER_PATH="$PROJECTS_ROOT/otterwatch-server"
MQTT_PATH="$PROJECTS_ROOT/otterwatch-mqtt"
AI_PATH="$PROJECTS_ROOT/otterwatch-ai"

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}OtterWatch Docker Image Builder${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""
echo -e "${GREEN}Configuration:${NC}"
echo -e "  Registry: ${YELLOW}${REGISTRY}${NC}"
echo -e "  Version:  ${YELLOW}${VERSION}${NC}"
echo -e "  Tool:     ${YELLOW}${BUILD_TOOL}${NC}"
echo ""

# Function to build an image
build_image() {
    local name=$1
    local path=$2
    local tag="${REGISTRY}/${name}:${VERSION}"
    
    echo -e "${BLUE}----------------------------------------${NC}"
    echo -e "${GREEN}Building: ${YELLOW}${name}${NC}"
    echo -e "${GREEN}Path:     ${YELLOW}${path}${NC}"
    echo -e "${GREEN}Tag:      ${YELLOW}${tag}${NC}"
    echo -e "${BLUE}----------------------------------------${NC}"
    
    if [ ! -d "$path" ]; then
        echo -e "${RED}Error: Directory not found: ${path}${NC}"
        return 1
    fi
    
    if [ ! -f "$path/Dockerfile" ]; then
        echo -e "${RED}Error: Dockerfile not found in: ${path}${NC}"
        return 1
    fi
    
    cd "$path"
    
    # Build the image
    if ! $BUILD_TOOL build -t "$tag" .; then
        echo -e "${RED}Error: Failed to build ${name}${NC}"
        return 1
    fi
    
    # Also tag as 'latest' if not already
    if [ "$VERSION" != "latest" ]; then
        $BUILD_TOOL tag "$tag" "${REGISTRY}/${name}:latest"
    fi
    
    echo -e "${GREEN}✓ Successfully built: ${tag}${NC}"
    echo ""
}

# Check if build tool is available
if ! command -v $BUILD_TOOL &> /dev/null; then
    echo -e "${RED}Error: $BUILD_TOOL is not installed${NC}"
    exit 1
fi

# Build each component
FAILED=0

if ! build_image "otterwatch-mqtt" "$MQTT_PATH"; then
    FAILED=$((FAILED + 1))
fi

if ! build_image "otterwatch-server" "$SERVER_PATH"; then
    FAILED=$((FAILED + 1))
fi

# Note: otterwatch-ai would need a Dockerfile first
# if ! build_image "otterwatch-ai" "$AI_PATH"; then
#     FAILED=$((FAILED + 1))
# fi

# Summary
echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}Build Summary${NC}"
echo -e "${BLUE}========================================${NC}"

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ All images built successfully!${NC}"
    echo ""
    echo -e "${GREEN}Built images:${NC}"
    echo -e "  - ${REGISTRY}/otterwatch-mqtt:${VERSION}"
    echo -e "  - ${REGISTRY}/otterwatch-server:${VERSION}"
    echo ""
    echo -e "${YELLOW}To push images to registry:${NC}"
    echo -e "  ${BUILD_TOOL} push ${REGISTRY}/otterwatch-mqtt:${VERSION}"
    echo -e "  ${BUILD_TOOL} push ${REGISTRY}/otterwatch-server:${VERSION}"
else
    echo -e "${RED}✗ ${FAILED} image(s) failed to build${NC}"
    exit 1
fi
