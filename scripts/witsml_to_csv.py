#!/usr/bin/env python3
"""Extract WITSML 1.4.1 time-indexed logs from Volve zip into Kaggle-format CSV.

Usage:
    python3 scripts/witsml_to_csv.py <well-pattern> [output.csv]

Example:
    python3 scripts/witsml_to_csv.py F-12 data/volve/F-12_witsml.csv
"""

import csv
import sys
import xml.etree.ElementTree as ET
import zipfile
from collections import defaultdict
from datetime import datetime, timezone

ZIP_PATH = "data/volve/Volve - Real Time drilling data 13.05.2018.zip"

# WITSML mnemonic → Kaggle column name mapping
# We map to Kaggle-format headers so the existing Volve loader works unchanged
KAGGLE_HEADER = [
    "Time Time",
    "Bit Depth m",
    "Total Depth m",
    "Weight on Bit kkgf",
    "Average Surface Torque kN.m",
    "Average Rotary Speed rpm",
    "Rate of Penetration m/h",
    "Stand Pipe Pressure kPa",
    "Total Hookload kkgf",
    "Mud Flow In L/min",
    "Mud Density In g/cm3",
    "Mud Density Out g/cm3",
    "Equivalent Circulating Density g/cm3",
    "Tmp In degC",
    "Temperature Out degC",
    "Gas (avg) %",
    "Total SPM 1/min",
    "Tank Volume (Active) m3",
    "Rig Mode",
    "Block Position m",
]

# Map WITSML mnemonics to Kaggle column index (0-based)
# Multiple mnemonics can map to the same column (first one wins per row)
MNEMONIC_MAP = {
    # Depth
    "DMEA": 1, "DBTM": 1, "DEPTH": 1, "BDEP": 1,
    "HDEP": 2, "HDEP_RT": 2,
    # WOB (kkgf in WITSML Kaggle format)
    "SWOB": 3, "WOB": 3, "WOBX": 3, "WOBA": 3,
    # Torque (kN.m)
    "TQA": 4, "TORQUE": 4, "TQX": 4, "STOR": 4, "RTOR": 4,
    # RPM
    "RPMA": 5, "RPM": 5, "RPMX": 5, "ROPA_RT": 5, "SRPM": 5,
    # ROP (m/h)
    "ROP5": 6, "ROPA": 6, "ROP": 6, "MROP": 6, "ROPA_5FT": 6,
    # SPP (kPa)
    "SPPA": 7, "SPP": 7, "PUMP": 7,
    # Hookload (kkgf)
    "HKLD": 8, "HKLA": 8, "HOOKLOAD": 8,
    # Flow in (L/min)
    "MFIA": 9, "FLOWIN": 9, "FLOW_IN": 9, "MFOP": 9,
    # MW in (g/cm3)
    "MDIA": 10, "MW_IN": 10, "MDENI": 10,
    # MW out (g/cm3)
    "MDOA": 10 + 1, "MW_OUT": 11, "MDENO": 11,
    # ECD (g/cm3)
    "ECDA": 12, "ECD": 12, "HECD": 12, "ECDB": 12,
    # Temp in (degC)
    "MTIA": 13, "MUD_TEMP_IN": 13, "MTIN": 13, "MTFA": 13,
    # Temp out (degC)
    "MTOA": 14, "MUD_TEMP_OUT": 14, "MTOUT": 14,
    # Gas (%)
    "G_TotIL": 15, "TOTAL_GAS": 15, "TOTGAS": 15,
    # SPM (1/min)
    "SPMA": 16, "SPM": 16, "STRATESUM": 16,
    # Pit volume (m3)
    "TVOLA": 17, "TANK_VOLUME": 17, "PIT_VOL": 17,
    # Block position (m)
    "BPOS": 19, "BLOCK_POS": 19,
}

NS = {"witsml": "http://www.witsml.org/schemas/1series"}


def parse_witsml_log(xml_bytes):
    """Parse a WITSML 1.4.1 log XML and return (mnemonic_list, rows).
    Each row is (timestamp_str, {col_index: value_str}).
    """
    root = ET.fromstring(xml_bytes)
    logs = root.findall("witsml:log", NS)
    if not logs:
        return []

    all_rows = []
    for log in logs:
        # Check if time-indexed
        idx_type = log.findtext("witsml:indexType", "", NS)
        if "time" not in idx_type.lower():
            continue

        # Get curve mnemonics
        curve_infos = log.findall("witsml:logCurveInfo", NS)
        mnemonics = []
        for ci in curve_infos:
            mnem = ci.findtext("witsml:mnemonic", "", NS).strip().upper()
            mnemonics.append(mnem)

        # Parse data rows
        log_data = log.find("witsml:logData", NS)
        if log_data is None:
            continue

        for data_node in log_data.findall("witsml:data", NS):
            text = data_node.text
            if not text:
                continue
            values = text.split(",")
            if len(values) < 2:
                continue

            timestamp = values[0].strip()
            row_data = {}
            for i, val in enumerate(values[1:], 1):
                val = val.strip()
                if not val:
                    continue
                if i < len(mnemonics):
                    mnem = mnemonics[i]
                else:
                    continue
                col_idx = MNEMONIC_MAP.get(mnem)
                if col_idx is not None and col_idx not in row_data:
                    row_data[col_idx] = val

            if row_data:
                all_rows.append((timestamp, row_data))

    return all_rows


def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} <well-pattern> [output.csv]")
        sys.exit(1)

    well_pattern = sys.argv[1].upper()
    output_path = sys.argv[2] if len(sys.argv) > 2 else f"data/volve/{sys.argv[1]}_witsml.csv"

    print(f"Extracting well pattern: {well_pattern}")
    print(f"Output: {output_path}")

    # Find matching time-indexed log files in zip
    all_rows = []
    with zipfile.ZipFile(ZIP_PATH, "r") as zf:
        matching = []
        for name in zf.namelist():
            name_upper = name.upper()
            if well_pattern.replace("-", "") in name_upper.replace("-", "").replace("_", "").replace("$47$", ""):
                if name.endswith(".xml") and "/log/" in name:
                    matching.append(name)

        print(f"Found {len(matching)} XML files matching '{well_pattern}'")

        for xml_path in sorted(matching):
            try:
                xml_bytes = zf.read(xml_path)
                rows = parse_witsml_log(xml_bytes)
                if rows:
                    print(f"  {xml_path}: {len(rows)} time rows")
                    all_rows.extend(rows)
            except Exception as e:
                print(f"  ERROR {xml_path}: {e}")

    if not all_rows:
        print("No time-indexed data found!")
        sys.exit(1)

    # Sort by timestamp
    all_rows.sort(key=lambda r: r[0])

    # Merge rows with same timestamp
    merged = defaultdict(dict)
    ts_order = []
    for ts, data in all_rows:
        if ts not in merged:
            ts_order.append(ts)
        for col_idx, val in data.items():
            if col_idx not in merged[ts]:
                merged[ts][col_idx] = val

    print(f"\nTotal unique timestamps: {len(ts_order)}")

    # Write CSV
    with open(output_path, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(KAGGLE_HEADER)

        rows_written = 0
        for ts in ts_order:
            row = [ts] + [""] * (len(KAGGLE_HEADER) - 1)
            data = merged[ts]
            for col_idx, val in data.items():
                if col_idx < len(row):
                    row[col_idx] = val
            writer.writerow(row)
            rows_written += 1

    print(f"Wrote {rows_written} rows to {output_path}")

    # Quick stats
    has_wob = sum(1 for ts in ts_order if 3 in merged[ts])
    has_rpm = sum(1 for ts in ts_order if 5 in merged[ts])
    has_rop = sum(1 for ts in ts_order if 6 in merged[ts])
    has_spp = sum(1 for ts in ts_order if 7 in merged[ts])
    print(f"Coverage: WOB={has_wob}, RPM={has_rpm}, ROP={has_rop}, SPP={has_spp}")


if __name__ == "__main__":
    main()
