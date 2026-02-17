#!/bin/bash
#
# OtterWatch Wrapper Script
# Manages the otterwatch service with automatic update and restart support.
#
# Exit codes from otterwatch:
#   0   - Normal exit
#   42  - Restart requested
#   43  - Update ready (new binary at otterwatch.new)
#
# Usage:
#   ./otterwatch-wrapper.sh
#
# Environment variables:
#   OTTERWATCH_BIN_DIR    - Binary directory (default: /usr/bin)
#   OTTERWATCH_CONFIG_DIR - Config directory (default: /etc/otterwatch)
#   OTTERWATCH_DATA_DIR   - Data directory (default: /var/lib/otterwatch)
#   OTTERWATCH_LOG        - Log file path (default: /var/log/otterwatch-wrapper.log)
#   OTTERWATCH_PORT       - HTTP port for health check (default: 8080)
#
# Can be used with systemd - see otterwatch.service
#

set -e

# Configuration
BINARY_NAME="otterwatch"
BINARY_DIR="${OTTERWATCH_BIN_DIR:-/usr/bin}"
CONFIG_DIR="${OTTERWATCH_CONFIG_DIR:-/etc/otterwatch}"
DATA_DIR="${OTTERWATCH_DATA_DIR:-/var/lib/otterwatch}"
LOG_FILE="${OTTERWATCH_LOG:-/var/log/otterwatch-wrapper.log}"

# Ensure log directory exists
mkdir -p "$(dirname "$LOG_FILE")" 2>/dev/null || true

# Exit codes
EXIT_RESTART=42
EXIT_UPDATE=43
EXIT_RECONNECT=44

# Health check settings
HEALTH_CHECK_RETRIES=5
HEALTH_CHECK_DELAY=2
HEALTH_CHECK_PORT="${OTTERWATCH_PORT:-8080}"

# Paths
BINARY_PATH="${BINARY_DIR}/${BINARY_NAME}"
NEW_BINARY_PATH="${BINARY_DIR}/${BINARY_NAME}.new"
BACKUP_BINARY_PATH="${BINARY_DIR}/${BINARY_NAME}.backup"

# Logging
log() {
    local timestamp=$(date '+%Y-%m-%d %H:%M:%S')
    echo "[$timestamp] $1" | tee -a "$LOG_FILE"
}

log_error() {
    log "ERROR: $1"
}

log_info() {
    log "INFO: $1"
}

# Health check - verify the agent is responding
health_check() {
    local retries=$HEALTH_CHECK_RETRIES
    local delay=$HEALTH_CHECK_DELAY

    log_info "Performing health check..."

    for ((i=1; i<=retries; i++)); do
        if curl -s -f "http://127.0.0.1:${HEALTH_CHECK_PORT}/system-stats" > /dev/null 2>&1; then
            log_info "Health check passed (attempt $i)"
            return 0
        fi
        log_info "Health check attempt $i failed, waiting ${delay}s..."
        sleep "$delay"
    done

    log_error "Health check failed after $retries attempts"
    return 1
}

# Replace current binary with new one
apply_update() {
    if [[ ! -f "$NEW_BINARY_PATH" ]]; then
        log_error "New binary not found at $NEW_BINARY_PATH"
        return 1
    fi

    # Create backup of current binary (if backup doesn't exist from download)
    if [[ -f "$BINARY_PATH" ]] && [[ ! -f "$BACKUP_BINARY_PATH" ]]; then
        log_info "Creating backup of current binary..."
        cp "$BINARY_PATH" "$BACKUP_BINARY_PATH"
    fi

    # Replace binary
    log_info "Replacing binary with new version..."
    mv "$NEW_BINARY_PATH" "$BINARY_PATH"
    chmod +x "$BINARY_PATH"

    log_info "Binary update applied successfully"
    return 0
}

# Rollback to backup binary
rollback() {
    if [[ ! -f "$BACKUP_BINARY_PATH" ]]; then
        log_error "No backup binary available for rollback"
        return 1
    fi

    log_info "Rolling back to previous version..."
    mv "$BACKUP_BINARY_PATH" "$BINARY_PATH"
    chmod +x "$BINARY_PATH"

    log_info "Rollback completed"
    return 0
}

# Cleanup after successful update
cleanup_after_update() {
    # Remove backup after successful health check
    if [[ -f "$BACKUP_BINARY_PATH" ]]; then
        log_info "Cleaning up backup binary..."
        rm -f "$BACKUP_BINARY_PATH"
    fi

    # Remove any leftover new binary
    if [[ -f "$NEW_BINARY_PATH" ]]; then
        rm -f "$NEW_BINARY_PATH"
    fi
}

# Main loop
main() {
    local just_updated=false
    local exit_code=0

    log_info "OtterWatch wrapper starting..."
    log_info "Binary: $BINARY_PATH"
    log_info "Config dir: $CONFIG_DIR"

    # Verify binary exists
    if [[ ! -x "$BINARY_PATH" ]]; then
        log_error "Binary not found or not executable: $BINARY_PATH"
        exit 1
    fi

    while true; do
        log_info "Starting OtterWatch..."

        # Run the binary
        set +e
        "$BINARY_PATH"
        exit_code=$?
        set -e

        log_info "OtterWatch exited with code: $exit_code"

        case $exit_code in
            0)
                # Normal exit
                log_info "Normal exit, stopping wrapper"
                exit 0
                ;;
            $EXIT_RESTART)
                # Restart requested
                log_info "Restart requested, restarting in 2 seconds..."
                sleep 2
                continue
                ;;
            $EXIT_UPDATE)
                # Update ready
                log_info "Update ready, applying..."

                if apply_update; then
                    just_updated=true
                    log_info "Update applied, restarting with new version..."
                    sleep 2

                    # Start new version and check health
                    "$BINARY_PATH" &
                    local pid=$!

                    sleep 3  # Give it time to start

                    if health_check; then
                        log_info "Update successful, new version is healthy"
                        cleanup_after_update

                        # Wait for the process we started
                        wait $pid
                        exit_code=$?
                        log_info "Process exited with code: $exit_code"

                        # Continue the loop based on exit code
                        case $exit_code in
                            0)
                                log_info "Normal exit after update"
                                exit 0
                                ;;
                            $EXIT_RESTART)
                                log_info "Restart requested after update"
                                sleep 2
                                continue
                                ;;
                            *)
                                log_info "Unexpected exit after update, restarting..."
                                sleep 2
                                continue
                                ;;
                        esac
                    else
                        log_error "Health check failed after update!"
                        kill $pid 2>/dev/null || true

                        if rollback; then
                            log_info "Rolled back to previous version, restarting..."
                            just_updated=false
                            sleep 2
                            continue
                        else
                            log_error "Rollback failed! Manual intervention required."
                            exit 1
                        fi
                    fi
                else
                    log_error "Failed to apply update, continuing with current version"
                    sleep 2
                    continue
                fi
                ;;
            $EXIT_RECONNECT)
                # Reconnect requested - restart immediately to re-run bootstrap
                log_info "Reconnect requested, restarting immediately..."
                sleep 1
                continue
                ;;
            *)
                # Unexpected exit - restart with backoff
                if $just_updated; then
                    log_error "Crash after update, attempting rollback..."
                    if rollback; then
                        just_updated=false
                        sleep 2
                        continue
                    else
                        log_error "Rollback failed after crash"
                        exit 1
                    fi
                fi

                log_info "Unexpected exit ($exit_code), restarting in 5 seconds..."
                sleep 5
                continue
                ;;
        esac
    done
}

# Handle signals
trap 'log_info "Received SIGTERM, stopping..."; exit 0' SIGTERM
trap 'log_info "Received SIGINT, stopping..."; exit 0' SIGINT

# Run main
main "$@"
