#!/usr/bin/env bash
# Build the ProFlow MCP server and configure it for Claude Code.
#
# Usage:
#   ./tools/setup_mcp.sh          # build + configure
#   ./tools/setup_mcp.sh --build  # build only
#   ./tools/setup_mcp.sh --check  # verify configuration

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BINARY_NAME="proflow_mcp"
MCP_CONFIG="$PROJECT_DIR/.mcp.json"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[✓]${NC} $*"; }
warn()  { echo -e "${YELLOW}[!]${NC} $*"; }
error() { echo -e "${RED}[✗]${NC} $*"; }

build_binary() {
    echo "Building $BINARY_NAME..."
    cargo build --release --bin "$BINARY_NAME" --manifest-path "$PROJECT_DIR/Cargo.toml"
    BINARY_PATH="$PROJECT_DIR/target/release/$BINARY_NAME"
    if [ -f "$BINARY_PATH" ]; then
        info "Built: $BINARY_PATH"
    else
        error "Build failed"
        exit 1
    fi
}

check_credentials() {
    local missing=0
    if [ -z "${PCO_APP_ID:-}" ]; then
        # Check .env file
        if [ -f "$PROJECT_DIR/.env" ] && grep -q "PCO_APP_ID" "$PROJECT_DIR/.env"; then
            : # found in .env
        else
            warn "PCO_APP_ID not set (set in environment or .env file)"
            missing=1
        fi
    fi
    if [ -z "${PCO_SECRET:-}" ]; then
        if [ -f "$PROJECT_DIR/.env" ] && grep -q "PCO_SECRET" "$PROJECT_DIR/.env"; then
            : # found in .env
        else
            warn "PCO_SECRET not set (set in environment or .env file)"
            missing=1
        fi
    fi
    return $missing
}

write_mcp_config() {
    BINARY_PATH="$PROJECT_DIR/target/release/$BINARY_NAME"

    # Build env block — only include vars that aren't in .env
    local env_json="{"
    local needs_comma=false

    # Always set PROFLOW_DATA so the binary finds bible/template data
    env_json+="\"PROFLOW_DATA\": \"$PROJECT_DIR/data\""
    needs_comma=true

    # Include credentials if they're in environment (not .env)
    if [ -n "${PCO_APP_ID:-}" ]; then
        $needs_comma && env_json+=","
        env_json+="\"PCO_APP_ID\": \"$PCO_APP_ID\""
        needs_comma=true
    fi
    if [ -n "${PCO_SECRET:-}" ]; then
        $needs_comma && env_json+=","
        env_json+="\"PCO_SECRET\": \"$PCO_SECRET\""
        needs_comma=true
    fi
    if [ -n "${LIBRARY_DIR:-}" ]; then
        $needs_comma && env_json+=","
        env_json+="\"LIBRARY_DIR\": \"$LIBRARY_DIR\""
    fi

    env_json+="}"

    cat > "$MCP_CONFIG" <<EOF
{
  "mcpServers": {
    "proflow": {
      "command": "$BINARY_PATH",
      "cwd": "$PROJECT_DIR",
      "env": $env_json
    }
  }
}
EOF
    info "Wrote $MCP_CONFIG"
}

check_config() {
    if [ ! -f "$MCP_CONFIG" ]; then
        error "No .mcp.json found. Run: ./tools/setup_mcp.sh"
        exit 1
    fi

    BINARY_PATH=$(python3 -c "import json; print(json.load(open('$MCP_CONFIG'))['mcpServers']['proflow']['command'])" 2>/dev/null || echo "")
    if [ -z "$BINARY_PATH" ] || [ ! -f "$BINARY_PATH" ]; then
        error "Binary not found at: $BINARY_PATH"
        error "Run: ./tools/setup_mcp.sh"
        exit 1
    fi

    info "Config: $MCP_CONFIG"
    info "Binary: $BINARY_PATH"

    if check_credentials; then
        info "Credentials: configured"
    fi

    # Quick smoke test — send initialize and check for response
    local INIT_MSG='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
    local RESULT
    RESULT=$(printf '%s\n' "$INIT_MSG" | "$BINARY_PATH" 2>/dev/null &
        local PID=$!
        sleep 2
        kill "$PID" 2>/dev/null
        wait "$PID" 2>/dev/null
    )
    if echo "$RESULT" | grep -q "tools"; then
        info "Smoke test: server responds"
    else
        warn "Smoke test: could not verify (may need credentials)"
    fi
}

case "${1:-}" in
    --build)
        build_binary
        ;;
    --check)
        check_config
        ;;
    *)
        build_binary
        echo ""
        if ! check_credentials; then
            echo ""
            warn "Configure credentials before using:"
            warn "  export PCO_APP_ID=your_app_id"
            warn "  export PCO_SECRET=your_secret"
            warn "  Or add them to $PROJECT_DIR/.env"
            echo ""
        fi
        write_mcp_config
        echo ""
        info "MCP server configured. Restart Claude Code to pick up the new server."
        ;;
esac
