#!/usr/bin/env bash
# fleet-rig-setup.sh — Complete SAIREN rig setup + enrollment
# Usage: ./scripts/fleet-rig-setup.sh

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
DEFAULT_CONFIG_DIR="$PROJECT_DIR/config"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

info()    { echo -e "${BLUE}[info]${NC} $*"; }
success() { echo -e "${GREEN}[ok]${NC} $*"; }
warn()    { echo -e "${YELLOW}[warn]${NC} $*"; }
error()   { echo -e "${RED}[error]${NC} $*"; }
header()  { echo -e "\n${BOLD}=== $* ===${NC}\n"; }

# ─── Detect existing enrollment ────────────────────────────────────────────────

find_config_dir() {
    # Check common locations for an existing enrollment
    for dir in "$DEFAULT_CONFIG_DIR" /etc/sairen-os; do
        if [[ -f "$dir/env" ]]; then
            echo "$dir"
            return
        fi
    done
    echo ""
}

load_enrollment() {
    local dir="$1"
    if [[ -f "$dir/env" ]]; then
        # shellcheck source=/dev/null
        source "$dir/env"
    fi
}

# ─── Step 1: Build ─────────────────────────────────────────────────────────────

build_rig() {
    header "Step 1/3: Build sairen-os"

    local binary="$PROJECT_DIR/target/release/sairen-os"

    if [[ -x "$binary" ]]; then
        # Verify it was built with fleet-client
        if "$binary" --help 2>&1 | grep -q "enroll"; then
            success "sairen-os binary exists with fleet-client support"
            read -rp "$(echo -e "${YELLOW}Rebuild? [y/N]:${NC} ")" ans
            if [[ ! "$ans" =~ ^[Yy] ]]; then
                return
            fi
        else
            warn "Binary exists but missing fleet-client feature. Rebuilding..."
        fi
    fi

    info "Building sairen-os with fleet-client (release)... this may take a few minutes"
    (cd "$PROJECT_DIR" && cargo build --release --features fleet-client) || {
        error "Build failed. Check Rust toolchain and dependencies."
        exit 1
    }
    success "Build complete: $binary"
}

# ─── Step 2: Enroll ────────────────────────────────────────────────────────────

enroll_rig() {
    header "Step 2/3: Enroll with Fleet Hub"

    read -rp "$(echo -e "${YELLOW}Hub URL (e.g. http://hub-ip:8080):${NC} ")" hub_url
    read -rp "$(echo -e "${YELLOW}Fleet passphrase:${NC} ")" passphrase
    read -rp "$(echo -e "${YELLOW}Rig ID:${NC} ")" rig_id
    read -rp "$(echo -e "${YELLOW}Well ID:${NC} ")" well_id
    read -rp "$(echo -e "${YELLOW}Field:${NC} ")" field
    read -rp "$(echo -e "${YELLOW}Config directory [$DEFAULT_CONFIG_DIR]:${NC} ")" config_dir
    config_dir="${config_dir:-$DEFAULT_CONFIG_DIR}"

    if [[ -z "$hub_url" || -z "$passphrase" || -z "$rig_id" || -z "$well_id" || -z "$field" ]]; then
        error "Hub URL, passphrase, rig ID, well ID, and field are all required."
        return 1
    fi

    # Strip trailing slash
    hub_url="${hub_url%/}"

    # Verify hub is reachable
    info "Checking hub connectivity..."
    if curl -sf "${hub_url}/api/fleet/health" &>/dev/null; then
        success "Hub reachable at $hub_url"
    else
        warn "Cannot reach hub at $hub_url — enrollment may still work if it comes online"
    fi

    # Create config dir if needed
    mkdir -p "$config_dir"

    local binary="$PROJECT_DIR/target/release/sairen-os"
    info "Enrolling with hub..."
    echo

    "$binary" enroll --hub "$hub_url" --passphrase "$passphrase" --rig-id "$rig_id" --well-id "$well_id" --field "$field" --config-dir "$config_dir"
    local rc=$?

    echo
    if [[ $rc -eq 0 ]]; then
        success "Enrollment complete!"
        success "Config written to: $config_dir/env"
        CONFIG_DIR="$config_dir"
    else
        error "Enrollment failed (exit code $rc)"
        error "Check the hub URL and passphrase, and try again."
        return 1
    fi
}

# ─── Step 3: Start ─────────────────────────────────────────────────────────────

start_rig() {
    local config_dir="$1"

    header "Starting SAIREN-OS"

    if [[ ! -f "$config_dir/env" ]]; then
        error "No env file found at $config_dir/env"
        error "Run enrollment first."
        return 1
    fi

    local binary="$PROJECT_DIR/target/release/sairen-os"
    if [[ ! -x "$binary" ]]; then
        error "Binary not found: $binary"
        return 1
    fi

    # Source the env file to display info
    # shellcheck source=/dev/null
    source "$config_dir/env"

    info "Rig ID:    ${FLEET_RIG_ID:-unknown}"
    info "Well ID:   ${WELL_ID:-unknown}"
    info "Hub URL:   ${FLEET_HUB_URL:-unknown}"
    info "Config:    $config_dir"
    echo
    info "Press Ctrl+C to stop"
    echo

    # Source env and run with well config from the config dir
    (
        set -a
        # shellcheck source=/dev/null
        source "$config_dir/env"
        set +a

        export RUST_LOG="${RUST_LOG:-info}"

        cd "$PROJECT_DIR"

        # Point SAIREN_CONFIG at the enrolled well_config.toml if it exists
        if [[ -f "$config_dir/well_config.toml" ]]; then
            export SAIREN_CONFIG="$config_dir/well_config.toml"
        fi

        exec "$binary" --stdin
    )
}

# ─── Show Config ────────────────────────────────────────────────────────────────

show_config() {
    local config_dir="$1"

    header "Rig Configuration"

    if [[ -f "$config_dir/env" ]]; then
        echo -e "${BOLD}$config_dir/env:${NC}"
        # Show env vars but mask the passphrase
        while IFS= read -r line; do
            if [[ "$line" =~ ^FLEET_PASSPHRASE= ]]; then
                local val="${line#FLEET_PASSPHRASE=}"
                echo "  FLEET_PASSPHRASE=${val:0:8}..."
            elif [[ -n "$line" && ! "$line" =~ ^# ]]; then
                echo "  $line"
            fi
        done < "$config_dir/env"
    else
        warn "No env file at $config_dir/env"
    fi

    echo
    if [[ -f "$config_dir/well_config.toml" ]]; then
        echo -e "${BOLD}$config_dir/well_config.toml:${NC}"
        head -20 "$config_dir/well_config.toml" | sed 's/^/  /'
        local lines
        lines=$(wc -l < "$config_dir/well_config.toml")
        if [[ $lines -gt 20 ]]; then
            echo "  ... ($lines lines total)"
        fi
    fi
}

# ─── Returning Menu ────────────────────────────────────────────────────────────

rig_menu() {
    local config_dir="$1"

    # Load to display info
    load_enrollment "$config_dir"

    while true; do
        echo
        echo -e "${BOLD}SAIREN Rig — Setup Menu${NC}"
        echo -e "  Rig: ${FLEET_RIG_ID:-unknown}  |  Hub: ${FLEET_HUB_URL:-unknown}"
        echo
        echo "  1) Start Rig"
        echo "  2) Re-enroll (new token)"
        echo "  3) Show Config"
        echo "  4) Quit"
        echo
        read -rp "$(echo -e "${YELLOW}Choose [1-4]:${NC} ")" choice

        case "$choice" in
            1) start_rig "$config_dir" ;;
            2) enroll_rig && load_enrollment "$CONFIG_DIR" ;;
            3) show_config "$config_dir" ;;
            4) echo "Bye."; exit 0 ;;
            *) warn "Invalid choice" ;;
        esac
    done
}

# ─── Main ───────────────────────────────────────────────────────────────────────

main() {
    echo -e "${BOLD}SAIREN Rig Setup${NC}"
    echo -e "${BLUE}Project: $PROJECT_DIR${NC}"
    echo

    # Check for existing enrollment
    local existing_dir
    existing_dir="$(find_config_dir)"

    if [[ -n "$existing_dir" ]]; then
        load_enrollment "$existing_dir"
        success "Existing enrollment found in $existing_dir"
        info "Rig ID: ${FLEET_RIG_ID:-unknown}"
        info "Hub:    ${FLEET_HUB_URL:-unknown}"
        rig_menu "$existing_dir"
    else
        info "No existing enrollment found. Running first-time setup..."
        echo
        build_rig
        enroll_rig || exit 1

        echo
        read -rp "$(echo -e "${YELLOW}Start the rig now? [Y/n]:${NC} ")" ans
        if [[ ! "$ans" =~ ^[Nn] ]]; then
            start_rig "${CONFIG_DIR:-$DEFAULT_CONFIG_DIR}"
        else
            info "Run this script again to start or manage the rig."
        fi
    fi
}

main "$@"
