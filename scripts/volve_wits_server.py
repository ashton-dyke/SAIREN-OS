#!/usr/bin/env python3
"""
Volve WITS Level 0 TCP Server

Reads Volve field CSV data and serves it as WITS Level 0 frames over TCP.
SAIREN-OS connects via: cargo run -- --wits-tcp 127.0.0.1:10001

Usage:
    # Serve F-9A at 1 record/second (default):
    python scripts/volve_wits_server.py --file data/volve/F-9A_witsml.csv

    # Serve F-12 at 10x speed:
    python scripts/volve_wits_server.py --file data/volve/F-12_witsml.csv --speed 10

    # Serve F-5 on custom port:
    python scripts/volve_wits_server.py --file data/volve/F-5_witsml.csv --port 10002

    # List available wells:
    python scripts/volve_wits_server.py --list
"""

import argparse
import asyncio
import csv
import glob
import os
import signal
import sys
import time
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

# ============================================================================
# Unit conversions (metric -> oilfield, matching src/volve.rs)
# ============================================================================

M_TO_FT = 3.28084
KKGF_TO_KLBF = 2.20462       # kilo-kilogram-force -> kilo-pounds
KNM_TO_KFTLB = 0.737562      # kilo-Newton-metres -> kilo-foot-pounds
MH_TO_FTHR = 3.28084         # metres/hour -> feet/hour
KPA_TO_PSI = 0.145038        # kilopascals -> PSI
LMIN_TO_GPM = 0.264172       # litres/min -> gallons/min
GCM3_TO_PPG = 8.34540        # grams/cm3 -> pounds/gallon
M3_TO_BBL = 6.28981          # cubic metres -> barrels


def celsius_to_fahrenheit(c: float) -> float:
    if c == 0.0:
        return 0.0
    return c * 9.0 / 5.0 + 32.0


# ============================================================================
# WITS Level 0 channel codes (Record 01 — time-based drilling data)
# ============================================================================

WITS_BLOCK_POS  = "0105"
WITS_BIT_DEPTH  = "0108"
WITS_HOLE_DEPTH = "0110"
WITS_ROP        = "0113"
WITS_HOOK_LOAD  = "0114"
WITS_WOB        = "0116"
WITS_RPM        = "0117"
WITS_TORQUE     = "0118"
WITS_SPP        = "0119"
WITS_PUMP_SPM   = "0120"
WITS_FLOW_IN    = "0121"
WITS_FLOW_OUT   = "0122"
WITS_PIT_VOL    = "0123"
WITS_MW_IN      = "0124"
WITS_MW_OUT     = "0125"
WITS_TEMP_IN    = "0126"
WITS_TEMP_OUT   = "0127"
WITS_GAS        = "0140"
WITS_ECD        = "0150"


# ============================================================================
# CSV column mapping (Kaggle / Volve WITSML format)
# ============================================================================

@dataclass
class ColumnMap:
    time: int = -1
    bit_depth: int = -1
    hole_depth: int = -1
    wob: int = -1
    torque: int = -1
    rpm: int = -1
    rop: int = -1
    spp: int = -1
    hook_load: int = -1
    flow_in: int = -1
    mw_in: int = -1
    mw_out: int = -1
    ecd: int = -1
    temp_in: int = -1
    temp_out: int = -1
    gas: int = -1
    pump_spm: int = -1
    pit_volume: int = -1
    block_pos: int = -1


def map_columns(header: list[str]) -> ColumnMap:
    """Map CSV header columns to field indices (Kaggle format)."""
    m = ColumnMap()
    for i, col in enumerate(header):
        cl = col.strip().lower()
        if cl == "time time" or cl == "datetime parsed":
            m.time = i
        elif cl.startswith("bit depth"):
            m.bit_depth = i
        elif cl.startswith("total depth") or cl.startswith("hole depth"):
            m.hole_depth = i
        elif cl.startswith("weight on bit"):
            m.wob = i
        elif cl.startswith("average surface torque"):
            m.torque = i
        elif cl.startswith("average rotary speed"):
            m.rpm = i
        elif cl.startswith("rate of penetration"):
            m.rop = i
        elif cl.startswith("stand pipe pressure"):
            m.spp = i
        elif cl.startswith("total hookload"):
            m.hook_load = i
        elif cl.startswith("mud flow in"):
            m.flow_in = i
        elif cl.startswith("mud density in"):
            m.mw_in = i
        elif cl.startswith("mud density out"):
            m.mw_out = i
        elif cl.startswith("equivalent circulating"):
            m.ecd = i
        elif cl.startswith("tmp in"):
            m.temp_in = i
        elif cl.startswith("temperature out"):
            m.temp_out = i
        elif cl.startswith("gas (avg)"):
            m.gas = i
        elif cl.startswith("total spm"):
            m.pump_spm = i
        elif cl.startswith("tank volume"):
            m.pit_volume = i
        elif cl.startswith("block position"):
            m.block_pos = i
    return m


def safe_float(fields: list[str], idx: int) -> float:
    """Extract float from CSV field, returning 0.0 for missing/NaN."""
    if idx < 0 or idx >= len(fields):
        return 0.0
    s = fields[idx].strip()
    if not s or s.lower() in ("nan", "null", "-", ""):
        return 0.0
    try:
        v = float(s)
        if v != v:  # NaN check
            return 0.0
        return v
    except ValueError:
        return 0.0


# ============================================================================
# Data loading
# ============================================================================

@dataclass
class WitsRecord:
    """A single drilling data record in oilfield units, ready for WITS0 framing."""
    bit_depth: float = 0.0      # ft
    hole_depth: float = 0.0     # ft
    wob: float = 0.0            # klbs
    torque: float = 0.0         # kft-lbs
    rpm: float = 0.0            # RPM
    rop: float = 0.0            # ft/hr
    hook_load: float = 0.0      # klbs
    spp: float = 0.0            # psi
    flow_in: float = 0.0        # gpm
    mw_in: float = 0.0          # ppg
    mw_out: float = 0.0         # ppg
    ecd: float = 0.0            # ppg
    temp_in: float = 0.0        # degF
    temp_out: float = 0.0       # degF
    gas: float = 0.0            # %
    pump_spm: float = 0.0       # 1/min
    pit_volume: float = 0.0     # bbl
    block_pos: float = 0.0      # ft

    def to_wits_frame(self) -> bytes:
        """Build a WITS Level 0 frame (&&...!! delimited)."""
        lines = ["&&"]
        items = [
            (WITS_BIT_DEPTH,  self.bit_depth),
            (WITS_HOLE_DEPTH, self.hole_depth),
            (WITS_ROP,        self.rop),
            (WITS_HOOK_LOAD,  self.hook_load),
            (WITS_WOB,        self.wob),
            (WITS_RPM,        self.rpm),
            (WITS_TORQUE,     self.torque),
            (WITS_SPP,        self.spp),
            (WITS_PUMP_SPM,   self.pump_spm),
            (WITS_FLOW_IN,    self.flow_in),
            (WITS_MW_IN,      self.mw_in),
            (WITS_MW_OUT,     self.mw_out),
            (WITS_ECD,        self.ecd),
            (WITS_TEMP_IN,    self.temp_in),
            (WITS_TEMP_OUT,   self.temp_out),
            (WITS_GAS,        self.gas),
            (WITS_PIT_VOL,    self.pit_volume),
            (WITS_BLOCK_POS,  self.block_pos),
        ]
        for code, value in items:
            lines.append(f"{code}{value:.2f}")
        lines.append("!!")
        return ("\r\n".join(lines) + "\r\n").encode("ascii")


def load_csv(filepath: str) -> list[WitsRecord]:
    """Load a Volve Kaggle-format CSV and convert to oilfield-unit WitsRecords."""
    records = []
    skipped = 0

    with open(filepath, "r", newline="", encoding="utf-8-sig") as f:
        reader = csv.reader(f)
        header = next(reader)
        col = map_columns(header)

        for row_num, fields in enumerate(reader, start=2):
            if not fields or all(f.strip() == "" for f in fields):
                continue

            # Read raw metric values
            raw_depth     = safe_float(fields, col.bit_depth)
            raw_hole      = safe_float(fields, col.hole_depth)
            raw_wob       = safe_float(fields, col.wob)
            raw_torque    = safe_float(fields, col.torque)
            raw_rpm       = safe_float(fields, col.rpm)
            raw_rop       = safe_float(fields, col.rop)
            raw_hookload  = safe_float(fields, col.hook_load)
            raw_spp       = safe_float(fields, col.spp)
            raw_flow_in   = safe_float(fields, col.flow_in)
            raw_mw_in     = safe_float(fields, col.mw_in)
            raw_mw_out    = safe_float(fields, col.mw_out)
            raw_ecd       = safe_float(fields, col.ecd)
            raw_temp_in   = safe_float(fields, col.temp_in)
            raw_temp_out  = safe_float(fields, col.temp_out)
            raw_gas       = safe_float(fields, col.gas)
            raw_pump_spm  = safe_float(fields, col.pump_spm)
            raw_pit_vol   = safe_float(fields, col.pit_volume)
            raw_block_pos = safe_float(fields, col.block_pos)

            # Skip all-zero rows (sensor feed gaps)
            if (abs(raw_wob) < 1e-10 and abs(raw_rpm) < 1e-10 and
                abs(raw_rop) < 1e-10 and abs(raw_spp) < 1e-10 and
                abs(raw_depth) < 1e-10):
                skipped += 1
                continue

            # Convert metric -> oilfield
            rec = WitsRecord(
                bit_depth  = raw_depth * M_TO_FT,
                hole_depth = (raw_hole if raw_hole else raw_depth) * M_TO_FT,
                wob        = raw_wob * KKGF_TO_KLBF,
                torque     = raw_torque * KNM_TO_KFTLB,
                rpm        = raw_rpm,
                rop        = raw_rop * MH_TO_FTHR,
                hook_load  = raw_hookload * KKGF_TO_KLBF,
                spp        = raw_spp * KPA_TO_PSI,
                flow_in    = raw_flow_in * LMIN_TO_GPM,
                mw_in      = raw_mw_in * GCM3_TO_PPG,
                mw_out     = raw_mw_out * GCM3_TO_PPG,
                ecd        = raw_ecd * GCM3_TO_PPG,
                temp_in    = celsius_to_fahrenheit(raw_temp_in),
                temp_out   = celsius_to_fahrenheit(raw_temp_out),
                gas        = raw_gas,
                pump_spm   = raw_pump_spm,
                pit_volume = raw_pit_vol * M3_TO_BBL,
                block_pos  = raw_block_pos * M_TO_FT,
            )
            records.append(rec)

    return records


# ============================================================================
# TCP Server
# ============================================================================

class WitsServer:
    def __init__(self, records: list[WitsRecord], speed: float, port: int, loop_replay: bool):
        self.records = records
        self.interval = 1.0 / speed  # seconds between frames
        self.port = port
        self.loop_replay = loop_replay
        self.clients: set[asyncio.StreamWriter] = set()
        self.running = True
        self.total_sent = 0

    async def handle_client(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
        addr = writer.get_extra_info("peername")
        print(f"  [+] Client connected: {addr}")
        self.clients.add(writer)
        try:
            # Keep the connection open until client disconnects
            while self.running:
                data = await reader.read(1024)
                if not data:
                    break
        except (ConnectionResetError, BrokenPipeError):
            pass
        finally:
            self.clients.discard(writer)
            writer.close()
            print(f"  [-] Client disconnected: {addr}")

    async def broadcast(self, data: bytes):
        """Send data to all connected clients."""
        dead = []
        for writer in self.clients:
            try:
                writer.write(data)
                await writer.drain()
            except (ConnectionResetError, BrokenPipeError, OSError):
                dead.append(writer)
        for w in dead:
            self.clients.discard(w)
            w.close()

    async def run(self):
        server = await asyncio.start_server(self.handle_client, "0.0.0.0", self.port)
        addr = server.sockets[0].getsockname()
        print(f"\n  WITS Level 0 server listening on {addr[0]}:{addr[1]}")
        print(f"  Connect SAIREN with: cargo run -- --wits-tcp 127.0.0.1:{self.port}")
        print(f"  Waiting for connection...\n")

        asyncio.create_task(self._feed_loop())

        async with server:
            await server.serve_forever()

    async def _feed_loop(self):
        """Stream WITS frames to all connected clients at the configured rate."""
        # Wait for at least one client
        while not self.clients and self.running:
            await asyncio.sleep(0.1)

        print(f"  >>> Streaming {len(self.records):,} records "
              f"(interval={self.interval:.3f}s)")
        print()

        pass_num = 0
        while self.running:
            pass_num += 1
            if pass_num > 1:
                print(f"\n  >>> Loop pass #{pass_num}")

            for i, rec in enumerate(self.records):
                if not self.running:
                    return
                if not self.clients:
                    # Wait for a client to reconnect
                    while not self.clients and self.running:
                        await asyncio.sleep(0.1)

                frame = rec.to_wits_frame()
                await self.broadcast(frame)
                self.total_sent += 1

                # Progress every 1000 records
                if self.total_sent % 1000 == 0:
                    depth_str = f"{rec.bit_depth:.0f}ft"
                    rop_str = f"ROP={rec.rop:.1f}"
                    wob_str = f"WOB={rec.wob:.1f}"
                    rpm_str = f"RPM={rec.rpm:.0f}"
                    print(f"  [{self.total_sent:>8,}] depth={depth_str:>8s}  "
                          f"{rop_str:>10s}  {wob_str:>9s}  {rpm_str:>7s}  "
                          f"clients={len(self.clients)}")

                await asyncio.sleep(self.interval)

            if not self.loop_replay:
                print(f"\n  >>> Replay complete. {self.total_sent:,} records sent.")
                print(f"  >>> Server stays open for SAIREN to finish processing.")
                # Keep server alive but stop sending
                while self.running:
                    await asyncio.sleep(1.0)
                return


# ============================================================================
# CLI
# ============================================================================

def list_wells(data_dir: str):
    """List available Volve CSV files."""
    pattern = os.path.join(data_dir, "*.csv")
    files = sorted(glob.glob(pattern))
    if not files:
        print(f"No CSV files found in {data_dir}")
        return

    print(f"\nAvailable wells in {data_dir}:\n")
    for f in files:
        size_mb = os.path.getsize(f) / (1024 * 1024)
        name = os.path.basename(f)
        # Quick line count estimate
        with open(f, "r") as fh:
            header = fh.readline()
            # Count a sample to estimate
            sample = fh.read(1024 * 100)
            lines_in_sample = sample.count("\n")
            bytes_in_sample = len(sample.encode())
            if bytes_in_sample > 0:
                total_lines = int((os.path.getsize(f) / bytes_in_sample) * lines_in_sample)
            else:
                total_lines = 0
        print(f"  {name:<30s}  {size_mb:6.1f} MB  ~{total_lines:>8,} records")

    print(f"\nUsage: python scripts/volve_wits_server.py --file data/volve/<name>.csv")


def main():
    parser = argparse.ArgumentParser(
        description="Volve WITS Level 0 TCP Server — streams real drilling data to SAIREN-OS"
    )
    parser.add_argument("--file", "-f", help="Path to Volve CSV file")
    parser.add_argument("--port", "-p", type=int, default=10001,
                        help="TCP port to listen on (default: 10001)")
    parser.add_argument("--speed", "-s", type=float, default=1.0,
                        help="Playback speed multiplier (default: 1.0 = 1 record/sec)")
    parser.add_argument("--loop", action="store_true",
                        help="Loop replay continuously")
    parser.add_argument("--list", action="store_true",
                        help="List available well CSV files")

    args = parser.parse_args()

    data_dir = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                            "data", "volve")

    if args.list:
        list_wells(data_dir)
        return

    if not args.file:
        # Default to first available CSV
        csvs = sorted(glob.glob(os.path.join(data_dir, "*.csv")))
        if not csvs:
            print("Error: No CSV files found. Use --file to specify a path.")
            sys.exit(1)
        args.file = csvs[0]
        print(f"No file specified, defaulting to: {args.file}")

    if not os.path.exists(args.file):
        print(f"Error: File not found: {args.file}")
        sys.exit(1)

    well_name = Path(args.file).stem
    print(f"\n{'='*60}")
    print(f"  Volve WITS0 Server")
    print(f"{'='*60}")
    print(f"  Well:     {well_name}")
    print(f"  File:     {args.file}")
    print(f"  Port:     {args.port}")
    print(f"  Speed:    {args.speed}x ({1.0/args.speed:.3f}s per record)")
    print(f"  Loop:     {'yes' if args.loop else 'no'}")

    print(f"\n  Loading CSV...")
    t0 = time.time()
    records = load_csv(args.file)
    elapsed = time.time() - t0

    if not records:
        print("Error: No valid records found in CSV.")
        sys.exit(1)

    # Summary stats
    depths = [r.bit_depth for r in records if r.bit_depth > 0]
    min_depth = min(depths) if depths else 0
    max_depth = max(depths) if depths else 0
    duration_at_speed = len(records) / args.speed

    print(f"  Loaded:   {len(records):,} records in {elapsed:.1f}s")
    print(f"  Depth:    {min_depth:.0f} - {max_depth:.0f} ft "
          f"({min_depth/M_TO_FT:.0f} - {max_depth/M_TO_FT:.0f} m)")
    print(f"  Duration: {duration_at_speed/3600:.1f} hours at {args.speed}x speed")

    # Run server
    srv = WitsServer(records, args.speed, args.port, args.loop)

    loop = asyncio.new_event_loop()

    def shutdown():
        srv.running = False
        print("\n  Shutting down...")

    loop.add_signal_handler(signal.SIGINT, shutdown)
    loop.add_signal_handler(signal.SIGTERM, shutdown)

    try:
        loop.run_until_complete(srv.run())
    except KeyboardInterrupt:
        shutdown()
    finally:
        loop.close()


if __name__ == "__main__":
    main()
