# SAIREN-OS Operations Runbook

Field engineer troubleshooting guide for the six most common failure scenarios.

---

## 1. Pipeline Stall — No Packets Processed

**Symptom:** `sairen_packets_total` counter is flat in Prometheus (or `total_analyses`
does not increase in `/api/v1/status`). No advisory tickets generated for > 5 minutes
during active drilling.

**Log signatures to look for:**

```
WARN sairen_os::pipeline: No WITS data received for 60s — check relay connection
```

or complete silence from the pipeline (no `info` log lines at all).

**Diagnosis steps:**

1. Check WITS relay is running and forwarding to the configured TCP port:
   ```bash
   ss -tn | grep 5000      # verify TCP connection to WITS relay
   journalctl -u sairen-os -n 50 --no-pager
   ```
2. Verify `WITS_HOST` and `WITS_PORT` in `/etc/sairen-os/env` match the rig relay.
3. If using CSV replay (`--csv` flag), confirm the file path is correct and readable.

**Remediation:**

- Restart the WITS relay process on the rig's data acquisition system.
- If the relay is up but no data flows, check that SAIREN-OS is started with the
  correct `--wits-tcp HOST:PORT` argument in `ExecStart`.
- If TCP connectivity is confirmed but packets still don't parse, enable debug
  logging: `RUST_LOG=sairen_os::acquisition=debug` and inspect the raw WITS frames.

---

## 2. Hub Unreachable — Fleet Sync Fails

**Symptom:** `fleet_queue_depth` in `/api/v1/status` is rising (edge is buffering
reports but cannot deliver them). Hub-originated intelligence is stale.

**Log signatures:**

```
WARN sairen_os::fleet: Hub sync failed — connection refused (http://192.168.1.179:8080)
WARN sairen_os::fleet: Fleet queue at 850 / 1000 — approaching capacity
```

**Diagnosis steps:**

1. Confirm `FLEET_HUB_URL` is set and reachable from the rig:
   ```bash
   curl -s http://<FLEET_HUB_URL>/api/fleet/health | jq .
   ```
2. Check hub process on the hub server:
   ```bash
   systemctl status fleet-hub
   journalctl -u fleet-hub -n 100 --no-pager
   ```
3. Verify network path (firewall, VPN, VLAN routing) between rig and hub.

**Remediation:**

- If hub is down, restart it: `systemctl restart fleet-hub`.
- If network path is broken, work with IT to restore connectivity.
- The edge continues drilling intelligence autonomously without the hub — the
  queue will drain automatically once connectivity is restored (up to 1 000 reports
  buffered).
- If the queue fills completely, oldest reports are dropped. Restart edge to clear
  and re-sync from the most recent 1 000 reports.

---

## 3. LLM Worker Backlog — Intelligence Jobs Pile Up

**Symptom:** `hub_intelligence_jobs_pending` in `/api/fleet/metrics` is growing
continuously (> 50 pending jobs that aren't clearing).

**Log signatures (on hub):**

```
WARN sairen_os::hub::intelligence: Intelligence scheduler — 120 pending jobs
ERROR sairen_os::llm: LLM inference failed: CUDA out of memory
```

**Diagnosis steps:**

1. Check GPU availability from the hub server:
   ```bash
   nvidia-smi
   ```
2. Inspect hub logs for inference errors:
   ```bash
   journalctl -u fleet-hub | grep -E "LLM|inference|CUDA|GPU"
   ```
3. Check available VRAM — mistralrs requires ≥ 8 GB free for the default model.

**Remediation:**

- If CUDA OOM: stop any other GPU workloads on the hub server and restart:
  ```bash
  systemctl restart fleet-hub
  ```
- If `nvidia-smi` shows the GPU is healthy but workers are stalled, the LLM model
  file may be corrupt. Re-download the model to `models/` and restart.
- If CUDA driver is outdated, update and reboot the hub server.
- As a temporary measure, intelligence workers degrade gracefully — the rig
  continues to operate with template-only advisories while the hub backlog clears.

---

## 4. SQLite / Sled DB Corruption

**Symptom:** Edge fails to start or crashes shortly after start with:

```
ERROR sairen_os::storage: Database not initialized: NotInitialized
ERROR sairen_os::storage::history: Failed to open sled tree: ...
```

or repeated `WARN poisoned RwLock` messages during operation.

**Diagnosis:**

The `./data/history.db` (SQLite) or `./data/sairen.db` (sled) file is corrupted,
typically due to an unclean shutdown (power loss, OOM kill).

**Remediation:**

```bash
systemctl stop sairen-os
# Back up the data directory first
cp -r /opt/sairen-os/data /opt/sairen-os/data.bak.$(date +%Y%m%d)
# Remove the corrupted databases (they will be recreated on next start)
rm -f /opt/sairen-os/data/history.db
rm -rf /opt/sairen-os/data/sairen.db
systemctl start sairen-os
journalctl -fu sairen-os   # confirm clean startup
```

Advisory history and acknowledgments from before the corruption will be lost.
ML insights and strategic reports are stored separately and may survive.

---

## 5. Log Rotation / Disk Space

**Symptom:** Disk usage on the rig computer is high. `df -h` shows the root
partition or `/var` near capacity.

**Diagnosis:**

```bash
df -h /var/log
journalctl --disk-usage
```

systemd journal is the primary log sink (configured in `sairen-os.service`).

**Remediation:**

Vacuum old journal entries to 500 MB:

```bash
journalctl --vacuum-size=500M
```

Or set a time-based limit:

```bash
journalctl --vacuum-time=7d
```

To prevent recurrence, edit `/etc/systemd/journald.conf`:

```ini
[Journal]
SystemMaxUse=500M
SystemKeepFree=200M
```

Then: `systemctl restart systemd-journald`

Also check the SAIREN-OS data directory for ML insight files:

```bash
du -sh /opt/sairen-os/data/
```

The SQLite DB is pruned to the last 30 days automatically on startup. For
faster disk recovery, delete old mid-well snapshots in the knowledge base:

```bash
find /opt/sairen-os/data -name "snapshot_*.toml*" -mtime +30 -delete
```

---

## 6. Admin Key Not Set in Production

**Symptom:** Hub refuses to start:

```
FATAL sairen_os::hub::config: FLEET_ADMIN_KEY must be set in release builds. \
      Set FLEET_ADMIN_KEY=<strong-secret> in the environment.
```

**Cause:** The `FLEET_ADMIN_KEY` environment variable is not set (or empty) in
the hub's systemd unit. In release builds the hub exits immediately — this is a
safety guard to prevent deployments with no admin authentication.

**Remediation:**

1. Generate a strong random key:
   ```bash
   openssl rand -hex 32
   ```
2. Add it to the hub's environment file (`/etc/fleet-hub/env` or inline in the
   service unit):
   ```ini
   Environment=FLEET_ADMIN_KEY=<hex-string-from-above>
   ```
   Or set it in `EnvironmentFile=/etc/fleet-hub/env`:
   ```bash
   echo "FLEET_ADMIN_KEY=$(openssl rand -hex 32)" >> /etc/fleet-hub/env
   chmod 600 /etc/fleet-hub/env
   ```
3. Reload and restart:
   ```bash
   systemctl daemon-reload
   systemctl start fleet-hub
   ```
4. Store the key in a secrets manager or password vault — it is required for all
   admin API calls (`Authorization: Bearer <FLEET_ADMIN_KEY>`).

> **Note:** In debug builds the hub falls back to a dev default and emits a `WARN`
> log — never deploy a debug build to production.

---

## Quick-Reference Log Patterns

| Pattern | What it means |
|---------|--------------|
| `Orchestrator voting complete` | Normal — ticket processed |
| `Advisory suppressed by CRITICAL cooldown` | Normal — rate limiting |
| `Hub sync failed` | Network issue between rig and hub |
| `Fleet queue at N / 1000` | Hub unreachable; queue filling |
| `LLM inference failed` | GPU/model issue on hub |
| `Database not initialized` | DB corruption — see scenario 4 |
| `FLEET_ADMIN_KEY must be set` | Missing env var — see scenario 6 |
| `Baseline locked — monitoring active` | Normal — learning phase complete |
