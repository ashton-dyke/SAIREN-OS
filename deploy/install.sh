#!/usr/bin/env bash
# SAIREN-OS Deployment Install Script
# Run as root on the rig-edge host
set -euo pipefail

INSTALL_DIR="/opt/sairen-os"
CONFIG_DIR="/etc/sairen-os"
DATA_DIR="${INSTALL_DIR}/data"
LOG_DIR="/var/log/sairen-os"
SERVICE_USER="sairen"

echo "=== SAIREN-OS Installer ==="

# Verify running as root
if [ "$(id -u)" -ne 0 ]; then
    echo "ERROR: Must run as root"
    exit 1
fi

# Create service user (no login shell)
if ! id -u "${SERVICE_USER}" &>/dev/null; then
    useradd --system --no-create-home --shell /usr/sbin/nologin "${SERVICE_USER}"
    echo "Created system user: ${SERVICE_USER}"
fi

# Create directory structure
mkdir -p "${INSTALL_DIR}/bin"
mkdir -p "${DATA_DIR}"
mkdir -p "${CONFIG_DIR}"
mkdir -p "${LOG_DIR}"

# Copy binary (must be pre-built with: cargo build --release)
BINARY="$(dirname "$0")/../target/release/sairen-os"
if [ -f "${BINARY}" ]; then
    cp "${BINARY}" "${INSTALL_DIR}/bin/sairen-os"
    chmod 755 "${INSTALL_DIR}/bin/sairen-os"
    echo "Installed binary to ${INSTALL_DIR}/bin/sairen-os"
else
    echo "WARNING: Binary not found at ${BINARY}"
    echo "  Build first with: cargo build --release"
    echo "  Then re-run this script"
fi

# Copy default config (only if no existing config)
if [ ! -f "${CONFIG_DIR}/well_config.toml" ]; then
    cp "$(dirname "$0")/../well_config.default.toml" "${CONFIG_DIR}/well_config.toml"
    echo "Installed default config to ${CONFIG_DIR}/well_config.toml"
    echo "  >>> EDIT THIS FILE for your well before starting the service <<<"
else
    echo "Config already exists at ${CONFIG_DIR}/well_config.toml (preserved)"
fi

# Create default env file (only if not present)
if [ ! -f "${CONFIG_DIR}/env" ]; then
    cat > "${CONFIG_DIR}/env" <<'EOF'
# SAIREN-OS Environment Configuration
# Uncomment and edit as needed

# WITS TCP server address
WITS_HOST=localhost
WITS_PORT=5000

# Log level: error, warn, info, debug, trace
RUST_LOG=info

# Server bind address (default: 0.0.0.0:8080)
# SAIREN_SERVER_ADDR=0.0.0.0:8080

# Campaign type: production (default) or pa
# CAMPAIGN=production

# Well and field identifiers (used by ML engine)
# WELL_ID=WELL-001
# FIELD_NAME=DEFAULT
EOF
    echo "Created env file at ${CONFIG_DIR}/env"
else
    echo "Env file already exists at ${CONFIG_DIR}/env (preserved)"
fi

# Set ownership
chown -R "${SERVICE_USER}:${SERVICE_USER}" "${INSTALL_DIR}"
chown -R "${SERVICE_USER}:${SERVICE_USER}" "${LOG_DIR}"
chown -R root:${SERVICE_USER} "${CONFIG_DIR}"
chmod 750 "${CONFIG_DIR}"
chmod 640 "${CONFIG_DIR}/well_config.toml" "${CONFIG_DIR}/env"

# Install systemd service
cp "$(dirname "$0")/sairen-os.service" /etc/systemd/system/sairen-os.service
systemctl daemon-reload
echo "Installed systemd service"

echo ""
echo "=== Installation Complete ==="
echo ""
echo "Next steps:"
echo "  1. Edit well config:  vi ${CONFIG_DIR}/well_config.toml"
echo "  2. Edit environment:  vi ${CONFIG_DIR}/env"
echo "  3. Enable service:    systemctl enable sairen-os"
echo "  4. Start service:     systemctl start sairen-os"
echo "  5. Check status:      systemctl status sairen-os"
echo "  6. View logs:         journalctl -u sairen-os -f"
