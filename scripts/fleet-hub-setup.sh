#!/usr/bin/env bash
# fleet-hub-setup.sh — Complete SAIREN Fleet Hub setup + admin tool
# Usage: ./scripts/fleet-hub-setup.sh

STATE_FILE="$HOME/.sairen-hub.env"
PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

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

# Load saved state if it exists
load_state() {
    if [[ -f "$STATE_FILE" ]]; then
        # shellcheck source=/dev/null
        source "$STATE_FILE"
    fi
}

save_var() {
    local key="$1" val="$2"
    if [[ -f "$STATE_FILE" ]] && grep -q "^${key}=" "$STATE_FILE" 2>/dev/null; then
        # Delete the old line and append the new one (avoids sed delimiter issues with URLs)
        grep -v "^${key}=" "$STATE_FILE" > "${STATE_FILE}.tmp"
        echo "${key}=${val}" >> "${STATE_FILE}.tmp"
        mv "${STATE_FILE}.tmp" "$STATE_FILE"
    else
        echo "${key}=${val}" >> "$STATE_FILE"
    fi
    chmod 600 "$STATE_FILE"
}

# ─── Step 1: PostgreSQL ────────────────────────────────────────────────────────

setup_postgres() {
    header "Step 1/5: PostgreSQL"

    if command -v psql &>/dev/null; then
        success "PostgreSQL client found: $(psql --version | head -1)"
    else
        warn "PostgreSQL not found."
        read -rp "$(echo -e "${YELLOW}Install postgresql? [Y/n]:${NC} ")" ans
        if [[ "$ans" =~ ^[Nn] ]]; then
            error "PostgreSQL is required. Install it and re-run."
            exit 1
        fi
        info "Installing postgresql..."
        sudo pacman -S --noconfirm postgresql || { error "pacman install failed"; exit 1; }
        success "PostgreSQL installed"
    fi

    # Check if cluster is initialized
    if [[ ! -d /var/lib/postgres/data ]] || [[ -z "$(ls -A /var/lib/postgres/data 2>/dev/null)" ]]; then
        info "Initializing PostgreSQL cluster..."
        sudo -u postgres initdb -D /var/lib/postgres/data || { error "initdb failed"; exit 1; }
        success "Cluster initialized"
    else
        success "PostgreSQL cluster already initialized"
    fi

    # Start service
    if systemctl is-active --quiet postgresql; then
        success "PostgreSQL service is running"
    else
        info "Starting PostgreSQL service..."
        sudo systemctl enable --now postgresql || { error "Failed to start PostgreSQL"; exit 1; }
        sleep 2
        success "PostgreSQL started"
    fi
}

# ─── Step 2: Database ──────────────────────────────────────────────────────────

setup_database() {
    header "Step 2/5: Database"

    local db_user="${DB_USER:-sairen}"
    local db_name="${DB_NAME:-sairen_fleet}"
    local db_pass

    # Check if database exists
    if sudo -u postgres psql -lqt 2>/dev/null | cut -d\| -f1 | grep -qw "$db_name"; then
        success "Database '$db_name' already exists"
    else
        info "Creating database user and database..."

        # Create user if not exists
        if sudo -u postgres psql -tAc "SELECT 1 FROM pg_roles WHERE rolname='$db_user'" 2>/dev/null | grep -q 1; then
            success "User '$db_user' already exists"
        else
            db_pass="$(openssl rand -base64 24 | tr -d '/+=' | head -c 24)"
            sudo -u postgres psql -c "CREATE ROLE $db_user WITH LOGIN PASSWORD '$db_pass';" || {
                error "Failed to create database user"; exit 1;
            }
            success "User '$db_user' created"
        fi

        sudo -u postgres psql -c "CREATE DATABASE $db_name OWNER $db_user;" || {
            error "Failed to create database"; exit 1;
        }
        success "Database '$db_name' created"
    fi

    # Build DATABASE_URL
    if [[ -n "$DATABASE_URL" ]]; then
        success "DATABASE_URL already set"
    else
        if [[ -z "$db_pass" ]]; then
            # User exists but we don't have the password — ask
            read -rsp "$(echo -e "${YELLOW}Enter password for DB user '$db_user' (or press Enter to reset it):${NC} ")" db_pass
            echo
            if [[ -z "$db_pass" ]]; then
                db_pass="$(openssl rand -base64 24 | tr -d '/+=' | head -c 24)"
                sudo -u postgres psql -c "ALTER ROLE $db_user WITH PASSWORD '$db_pass';" || {
                    error "Failed to reset password"; exit 1;
                }
                success "Password reset for '$db_user'"
            fi
        fi
        DATABASE_URL="postgres://${db_user}:${db_pass}@localhost/${db_name}"
        save_var "DATABASE_URL" "$DATABASE_URL"
        success "DATABASE_URL saved to $STATE_FILE"
    fi

    # Verify connectivity
    if psql "$DATABASE_URL" -c "SELECT 1" &>/dev/null; then
        success "Database connection verified"
    else
        error "Cannot connect to database with saved URL"
        error "URL: $DATABASE_URL"
        error "Try deleting $STATE_FILE and re-running, or fix PostgreSQL auth (pg_hba.conf)"
        exit 1
    fi
}

# ─── Step 3: Build ─────────────────────────────────────────────────────────────

build_hub() {
    header "Step 3/5: Build fleet-hub"

    local binary="$PROJECT_DIR/target/release/fleet-hub"

    if [[ -x "$binary" ]]; then
        success "fleet-hub binary exists: $binary"
        read -rp "$(echo -e "${YELLOW}Rebuild? [y/N]:${NC} ")" ans
        if [[ ! "$ans" =~ ^[Yy] ]]; then
            return
        fi
    fi

    info "Building fleet-hub (release)... this may take a few minutes"
    (cd "$PROJECT_DIR" && cargo build --release --features fleet-hub --bin fleet-hub) || {
        error "Build failed. Check Rust toolchain and dependencies."
        exit 1
    }
    success "Build complete: $binary"
}

# ─── Step 4: Passphrase ───────────────────────────────────────────────────────

setup_passphrase() {
    header "Step 4/5: Fleet Passphrase"

    if [[ -n "$FLEET_PASSPHRASE" ]]; then
        success "FLEET_PASSPHRASE already set (${#FLEET_PASSPHRASE} chars)"
        return
    fi

    echo -e "${BLUE}The passphrase is shared by the hub and all rigs (like a WiFi password).${NC}"
    read -rp "$(echo -e "${YELLOW}Enter a passphrase (or press Enter to generate one):${NC} ")" key

    if [[ -z "$key" ]]; then
        key="$(openssl rand -base64 32 | tr -d '/+=' | head -c 32)"
        info "Generated passphrase: $key"
    fi

    FLEET_PASSPHRASE="$key"
    save_var "FLEET_PASSPHRASE" "$FLEET_PASSPHRASE"
    success "FLEET_PASSPHRASE saved to $STATE_FILE"
}

# ─── Step 5: Start Hub ─────────────────────────────────────────────────────────

start_hub() {
    header "Starting Fleet Hub"

    local port="${HUB_PORT:-8080}"
    local binary="$PROJECT_DIR/target/release/fleet-hub"

    if [[ ! -x "$binary" ]]; then
        error "Binary not found: $binary"
        error "Run the build step first."
        return 1
    fi

    info "Hub URL:       http://localhost:${port}"
    info "Passphrase:    ${FLEET_PASSPHRASE:0:8}..."
    info "Database:      ${DATABASE_URL%%@*}@..."
    info "Migrations:    auto-applied on startup"
    echo
    info "Press Ctrl+C to stop the hub"
    echo

    (cd "$PROJECT_DIR" && \
        DATABASE_URL="$DATABASE_URL" \
        FLEET_PASSPHRASE="$FLEET_PASSPHRASE" \
        RUST_LOG="${RUST_LOG:-info,fleet_hub=debug}" \
        exec "$binary" --port "$port"
    )
}

# ─── Admin Menu ─────────────────────────────────────────────────────────────────

hub_url() {
    echo "http://localhost:${HUB_PORT:-8080}"
}

enroll_rig_menu() {
    header "Enroll Rig"

    read -rp "$(echo -e "${YELLOW}Rig ID:${NC} ")" rig_id
    read -rp "$(echo -e "${YELLOW}Well ID:${NC} ")" well_id
    read -rp "$(echo -e "${YELLOW}Field:${NC} ")" field

    if [[ -z "$rig_id" || -z "$well_id" || -z "$field" ]]; then
        error "rig_id, well_id, and field are all required"
        return 1
    fi

    local response rc
    response=$(curl -sf -X POST "$(hub_url)/api/fleet/enroll" \
        -H "Authorization: Bearer $FLEET_PASSPHRASE" \
        -H "Content-Type: application/json" \
        -d "{\"rig_id\":\"$rig_id\",\"well_id\":\"$well_id\",\"field\":\"$field\"}" 2>&1)
    rc=$?

    if [[ $rc -ne 0 ]]; then
        error "Failed to enroll rig. Is the hub running?"
        error "Response: $response"
        return 1
    fi

    echo
    success "Rig enrolled!"
    echo -e "${BOLD}Response:${NC}"
    echo "$response" | python3 -m json.tool 2>/dev/null || echo "$response"
}

list_rigs() {
    header "Enrolled Rigs"

    local response rc
    response=$(curl -sf "$(hub_url)/api/fleet/rigs" \
        -H "Authorization: Bearer $FLEET_PASSPHRASE" 2>&1)
    rc=$?

    if [[ $rc -ne 0 ]]; then
        error "Failed to list rigs. Is the hub running?"
        return 1
    fi

    echo "$response" | python3 -c "
import sys, json
rigs = json.load(sys.stdin)
if not rigs:
    print('  (no rigs enrolled)')
else:
    print(f'  {\"Rig ID\":<20} {\"Well\":<15} {\"Field\":<15} {\"Status\":<10} {\"Events\":<8} {\"Last Seen\"}')
    print(f'  {\"─\"*20} {\"─\"*15} {\"─\"*15} {\"─\"*10} {\"─\"*8} {\"─\"*20}')
    for r in rigs:
        rid = r.get('rig_id','?')
        well = r.get('well_id','?')
        field = r.get('field','?')
        status = r.get('status','?')
        evts = r.get('event_count', 0)
        seen = (r.get('last_seen') or 'never')[:19]
        print(f'  {rid:<20} {well:<15} {field:<15} {status:<10} {evts:<8} {seen}')
" 2>/dev/null || echo "$response"
}

health_check() {
    header "Health Check"

    local response rc
    response=$(curl -sf "$(hub_url)/api/fleet/health" 2>&1)
    rc=$?

    if [[ $rc -ne 0 ]]; then
        error "Hub not reachable at $(hub_url)"
        return 1
    fi

    echo "$response" | python3 -m json.tool 2>/dev/null || echo "$response"

    local status
    status=$(echo "$response" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status','unknown'))" 2>/dev/null)
    if [[ "$status" == "ok" ]]; then
        success "Hub is healthy"
    else
        warn "Hub status: $status"
    fi
}

reset_hub() {
    header "Reset Hub"

    echo -e "${RED}${BOLD}WARNING: This will drop the database and wipe saved config.${NC}"
    read -rp "$(echo -e "${YELLOW}Type 'RESET' to confirm:${NC} ")" confirm
    if [[ "$confirm" != "RESET" ]]; then
        info "Cancelled."
        return
    fi

    local db_name="${DB_NAME:-sairen_fleet}"
    local db_user="${DB_USER:-sairen}"

    info "Dropping database '$db_name'..."
    sudo -u postgres psql -c "DROP DATABASE IF EXISTS $db_name;" 2>/dev/null
    info "Recreating database..."
    sudo -u postgres psql -c "CREATE DATABASE $db_name OWNER $db_user;" 2>/dev/null

    info "Removing state file..."
    rm -f "$STATE_FILE"

    success "Reset complete. Run this script again to set up from scratch."
}

admin_menu() {
    while true; do
        echo
        echo -e "${BOLD}SAIREN Fleet Hub — Admin Menu${NC}"
        echo -e "  Hub: $(hub_url)  |  Passphrase: ${FLEET_PASSPHRASE:0:8}..."
        echo
        echo "  1) Start Hub"
        echo "  2) Enroll Rig"
        echo "  3) List Rigs"
        echo "  4) Health Check"
        echo "  5) Reset (drop DB + wipe config)"
        echo "  6) Quit"
        echo
        read -rp "$(echo -e "${YELLOW}Choose [1-6]:${NC} ")" choice

        case "$choice" in
            1) start_hub ;;
            2) enroll_rig_menu ;;
            3) list_rigs ;;
            4) health_check ;;
            5) reset_hub; return ;;
            6) echo "Bye."; exit 0 ;;
            *) warn "Invalid choice" ;;
        esac
    done
}

# ─── Main ───────────────────────────────────────────────────────────────────────

main() {
    echo -e "${BOLD}SAIREN Fleet Hub Setup${NC}"
    echo -e "${BLUE}Project: $PROJECT_DIR${NC}"
    echo

    load_state

    # Determine if initial setup is needed
    local needs_setup=false

    if ! command -v psql &>/dev/null; then
        needs_setup=true
    elif ! sudo -u postgres psql -lqt 2>/dev/null | cut -d\| -f1 | grep -qw "${DB_NAME:-sairen_fleet}"; then
        needs_setup=true
    elif [[ ! -x "$PROJECT_DIR/target/release/fleet-hub" ]]; then
        needs_setup=true
    elif [[ -z "$FLEET_PASSPHRASE" ]]; then
        needs_setup=true
    fi

    if [[ "$needs_setup" == true ]]; then
        info "Running first-time setup..."
        echo
        setup_postgres
        setup_database
        build_hub
        setup_passphrase

        echo
        success "Setup complete!"
        echo
        read -rp "$(echo -e "${YELLOW}Start the hub now? [Y/n]:${NC} ")" ans
        if [[ ! "$ans" =~ ^[Nn] ]]; then
            start_hub
        else
            info "Run this script again to access the admin menu."
        fi
    else
        success "Setup already complete"
        admin_menu
    fi
}

main "$@"
