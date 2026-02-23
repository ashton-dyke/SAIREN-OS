#!/usr/bin/env bash
# SAIREN Fleet Hub — Installation Script
# Run on the hub server as root
set -euo pipefail

echo "=== SAIREN Fleet Hub Installation ==="

# 1. Install PostgreSQL
echo "[1/7] Installing PostgreSQL..."
if ! command -v psql &>/dev/null; then
    apt-get update -qq
    apt-get install -y -qq postgresql postgresql-contrib
    systemctl enable postgresql
    systemctl start postgresql
fi

# 2. Create database and user
echo "[2/7] Setting up database..."
sudo -u postgres psql -tc "SELECT 1 FROM pg_roles WHERE rolname='sairen'" | grep -q 1 || \
    sudo -u postgres createuser sairen
sudo -u postgres psql -tc "SELECT 1 FROM pg_database WHERE datname='sairen_fleet'" | grep -q 1 || \
    sudo -u postgres createdb -O sairen sairen_fleet

# 3. Create sairen system user
echo "[3/7] Creating system user..."
if ! id -u sairen &>/dev/null; then
    useradd -r -s /bin/false sairen
fi

# 4. Install fleet-hub binary
echo "[4/7] Installing fleet-hub binary..."
if [ -f "./target/release/fleet-hub" ]; then
    cp ./target/release/fleet-hub /usr/local/bin/fleet-hub
    chmod +x /usr/local/bin/fleet-hub
else
    echo "ERROR: Build fleet-hub first: cargo build --release --bin fleet-hub --features fleet-hub"
    exit 1
fi

# 5. Generate admin key
echo "[5/7] Generating admin key..."
ADMIN_KEY=$(openssl rand -base64 32)
echo "ADMIN KEY (save this): ${ADMIN_KEY}"

# 6. Install systemd service
echo "[6/7] Installing systemd service..."
sed "s/FLEET_PASSPHRASE=changeme/FLEET_PASSPHRASE=${ADMIN_KEY}/" \
    deploy/fleet-hub.service > /etc/systemd/system/fleet-hub.service
systemctl daemon-reload

# 7. Start services
echo "[7/7] Starting Fleet Hub..."
systemctl enable fleet-hub
systemctl start fleet-hub

echo ""
echo "=== Fleet Hub installed successfully ==="
echo "  Admin key: ${ADMIN_KEY}"
echo "  Dashboard: http://$(hostname -I | awk '{print $1}'):8080/"
echo "  Health:    http://$(hostname -I | awk '{print $1}'):8080/api/fleet/health"
echo ""
echo "Next steps:"
echo "  1. Configure WireGuard (see deploy/wireguard/)"
echo "  2. Register rigs via: curl -X POST http://hub:8080/api/fleet/rigs/register"
echo "  3. Distribute API keys to rig operators"
