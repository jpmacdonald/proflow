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
    local binary_path="$PROJECT_DIR/target/release/$BINARY_NAME"
    local library_dir="${LIBRARY_DIR:-}"

    # Credentials intentionally stay in the process environment or the ignored
    # .env file. Never persist them in a repository-local MCP configuration.
    python3 - "$MCP_CONFIG" "$binary_path" "$PROJECT_DIR" "$library_dir" <<'PY'
import json
import os
import pathlib
import sys
import tempfile

target = pathlib.Path(sys.argv[1])
binary_path = sys.argv[2]
project_dir = pathlib.Path(sys.argv[3])
library_dir = sys.argv[4]

environment = {"PROFLOW_DATA": str(project_dir / "data")}
if library_dir:
    environment["LIBRARY_DIR"] = library_dir

config = {
    "mcpServers": {
        "proflow": {
            "command": binary_path,
            "cwd": str(project_dir),
            "env": environment,
        }
    }
}

target.parent.mkdir(parents=True, exist_ok=True)
temporary_path = None
try:
    with tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        dir=target.parent,
        prefix=f".{target.name}.",
        suffix=".tmp",
        delete=False,
    ) as temporary:
        temporary_path = pathlib.Path(temporary.name)
        os.fchmod(temporary.fileno(), 0o600)
        json.dump(config, temporary, indent=2)
        temporary.write("\n")
        temporary.flush()
        os.fsync(temporary.fileno())
    os.replace(temporary_path, target)
finally:
    if temporary_path is not None:
        temporary_path.unlink(missing_ok=True)
PY
    info "Wrote $MCP_CONFIG"
    if [ -n "${PCO_APP_ID:-}" ] || [ -n "${PCO_SECRET:-}" ]; then
        info "Credentials were not written; keep them in .env or the MCP host environment"
    fi
}

check_config() {
    if [ ! -f "$MCP_CONFIG" ]; then
        error "No .mcp.json found. Run: ./tools/setup_mcp.sh"
        exit 1
    fi

    BINARY_PATH=$(python3 - "$MCP_CONFIG" 2>/dev/null <<'PY' || true
import json
import pathlib
import sys

with pathlib.Path(sys.argv[1]).open(encoding="utf-8") as config_file:
    print(json.load(config_file)["mcpServers"]["proflow"]["command"])
PY
    )
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

    # Quick smoke test — initialize and exercise the public tool-list boundary.
    local INIT_MSG='{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}'
    local INITIALIZED_MSG='{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}'
    local LIST_TOOLS_MSG='{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
    local RESULT
    RESULT=$({ printf '%s\n' "$INIT_MSG"; printf '%s\n' "$INITIALIZED_MSG"; printf '%s\n' "$LIST_TOOLS_MSG"; } | "$BINARY_PATH" 2>/dev/null || true)
    if echo "$RESULT" | grep -q '"id":2' && echo "$RESULT" | grep -q '"tools"'; then
        info "Smoke test: server lists tools"
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
