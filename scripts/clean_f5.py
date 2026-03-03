#!/usr/bin/env python3
"""Clean F-5 WITSML CSV data for SAIREN pipeline processing.

Fixes:
  1. Forward-fill sparse rows (44.1% of rows have empty sensor columns)
  2. Interpolate negative hookload (3,456 rows, contiguous Circulating block)
  3. Replace extreme torque < -67.8 kN.m (132 rows) with forward-fill
  4. Clamp mild negative torque (-67.8, 0) to 0.0 (16,632 rows)
  5. Fix tank volume calibration offset (88,160 negative rows)
  6. Fill ECD from mud weight for Drilling/Circulating rows
  7. Forward-fill blank rig modes (184 rows)

Usage:
  python3 scripts/clean_f5.py
"""

import sys
from pathlib import Path

try:
    import pandas as pd
    import numpy as np
except ImportError:
    print("ERROR: pandas and numpy required.  pip install pandas numpy", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
INPUT = ROOT / "data" / "volve" / "F-5_witsml.csv"
OUTPUT = ROOT / "data" / "volve" / "F-5_witsml_clean.csv"

SENSOR_COLS = [
    "Weight on Bit kkgf",
    "Average Surface Torque kN.m",
    "Average Rotary Speed rpm",
    "Rate of Penetration m/h",
    "Stand Pipe Pressure kPa",
    "Total Hookload kkgf",
    "Mud Flow In L/min",
    "Mud Density In g/cm3",
    "Mud Density Out g/cm3",
    "Tmp In degC",
    "Temperature Out degC",
    "Gas (avg) %",
    "Total SPM 1/min",
    "Tank Volume (Active) m3",
]

# Columns that must be numeric for cleaning operations
NUMERIC_COLS = SENSOR_COLS + [
    "Bit Depth m",
    "Total Depth m",
    "Equivalent Circulating Density g/cm3",
    "Block Position m",
]


def load(path: Path) -> pd.DataFrame:
    """Load CSV with dtype=str to distinguish empty cells from '0'."""
    print(f"Loading {path} ...")
    df = pd.read_csv(path, dtype=str)
    print(f"  {len(df):,} rows, {len(df.columns)} columns")
    return df


def to_numeric(df: pd.DataFrame) -> None:
    """Convert numeric columns in-place, coercing errors to NaN."""
    for col in NUMERIC_COLS:
        if col in df.columns:
            df[col] = pd.to_numeric(df[col], errors="coerce")


def step1_forward_fill_sparse(df: pd.DataFrame) -> int:
    """Forward-fill sensor values into sparse rows.

    Sparse rows have Bit Depth + Total Depth but all sensor columns empty.
    These come from an interleaved WITSML stream at ~2.7s spacing.
    """
    sparse_mask = df[SENSOR_COLS].isna().all(axis=1)
    n = sparse_mask.sum()
    if n == 0:
        print("  Step 1: No sparse rows found (skipping)")
        return 0

    # Forward-fill only the sensor columns (plus Block Position)
    fill_cols = SENSOR_COLS + ["Block Position m"]
    for col in fill_cols:
        if col in df.columns:
            df[col] = df[col].ffill()

    print(f"  Step 1: Forward-filled {n:,} sparse rows")
    return n


def step2_interpolate_neg_hookload(df: pd.DataFrame) -> int:
    """Replace negative hookload with linear interpolation."""
    col = "Total Hookload kkgf"
    neg_mask = df[col] < 0
    n = neg_mask.sum()
    if n == 0:
        print("  Step 2: No negative hookload (skipping)")
        return 0

    # Preserve original NaN positions so interpolation doesn't fill sparse rows
    was_nan = df[col].isna()
    df.loc[neg_mask, col] = np.nan
    df[col] = df[col].interpolate(method="linear")
    df[col] = df[col].bfill()  # edge case: negatives at start
    df.loc[was_nan, col] = np.nan  # restore original gaps

    print(f"  Step 2: Interpolated {n:,} negative hookload rows")
    return n


def step3_fix_extreme_torque(df: pd.DataFrame) -> int:
    """Replace extreme torque (< -67.8 kN.m) with forward-fill."""
    col = "Average Surface Torque kN.m"
    threshold = -67.8
    extreme_mask = df[col] < threshold
    n = extreme_mask.sum()
    if n == 0:
        print("  Step 3: No extreme torque (skipping)")
        return 0

    # Preserve original NaN positions so ffill doesn't fill sparse rows
    was_nan = df[col].isna()
    df.loc[extreme_mask, col] = np.nan
    df[col] = df[col].ffill()
    df[col] = df[col].bfill()  # safety for leading NaN
    df.loc[was_nan, col] = np.nan  # restore original gaps

    print(f"  Step 3: Forward-filled {n:,} extreme torque rows")
    return n


def step4_clamp_mild_neg_torque(df: pd.DataFrame) -> int:
    """Clamp mild negative torque (-67.8, 0) to 0.0."""
    col = "Average Surface Torque kN.m"
    mild_mask = (df[col] > -67.8) & (df[col] < 0)
    n = mild_mask.sum()
    if n == 0:
        print("  Step 4: No mild negative torque (skipping)")
        return 0

    df.loc[mild_mask, col] = 0.0

    print(f"  Step 4: Clamped {n:,} mild negative torque rows to 0.0")
    return n


def step5_fix_tank_volume(df: pd.DataFrame, offset: float) -> int:
    """Shift tank volume by pre-computed offset to correct calibration bias."""
    col = "Tank Volume (Active) m3"
    valid = df[col].dropna()
    if len(valid) == 0:
        print("  Step 5: No valid tank volume data (skipping)")
        return 0

    neg_count_before = (df[col] < 0).sum()
    df[col] = df[col] + offset
    neg_count_after = (df[col] < 0).sum()

    print(f"  Step 5: Shifted tank volume by +{offset:.2f} m3 "
          f"(negative rows: {neg_count_before:,} -> {neg_count_after:,})")
    return neg_count_before


def step6_fill_ecd(df: pd.DataFrame) -> int:
    """Fill ECD from mud weight in for Drilling/Circulating rows."""
    ecd_col = "Equivalent Circulating Density g/cm3"
    mw_col = "Mud Density In g/cm3"
    mode_col = "Rig Mode"

    if ecd_col not in df.columns:
        print("  Step 6: ECD column not found (skipping)")
        return 0

    active_modes = df[mode_col].isin(["Drilling", "Circulating"])
    valid_mw = df[mw_col].notna() & (df[mw_col] > 0)
    fill_mask = active_modes & valid_mw & df[ecd_col].isna()
    n = fill_mask.sum()

    df.loc[fill_mask, ecd_col] = df.loc[fill_mask, mw_col]

    print(f"  Step 6: Filled {n:,} ECD values from mud weight")
    return n


def step7_fill_rig_mode(df: pd.DataFrame) -> int:
    """Forward-fill blank rig modes."""
    col = "Rig Mode"
    blank_mask = df[col].isna() | (df[col] == "")
    n = blank_mask.sum()
    if n == 0:
        print("  Step 7: No blank rig modes (skipping)")
        return 0

    df.loc[blank_mask, col] = np.nan
    df[col] = df[col].ffill()

    print(f"  Step 7: Forward-filled {n:,} blank rig modes")
    return n


def save(df: pd.DataFrame, path: Path, original_header: str) -> None:
    """Save cleaned CSV, rounding numeric columns to avoid float artifacts."""
    print(f"\nSaving {path} ...")

    # Round numeric columns to reasonable precision
    round_map = {
        "Bit Depth m": 10,
        "Total Depth m": 10,
        "Weight on Bit kkgf": 6,
        "Average Surface Torque kN.m": 6,
        "Average Rotary Speed rpm": 6,
        "Rate of Penetration m/h": 6,
        "Stand Pipe Pressure kPa": 6,
        "Total Hookload kkgf": 10,
        "Mud Flow In L/min": 6,
        "Mud Density In g/cm3": 11,
        "Mud Density Out g/cm3": 11,
        "Equivalent Circulating Density g/cm3": 11,
        "Tmp In degC": 10,
        "Temperature Out degC": 10,
        "Gas (avg) %": 6,
        "Total SPM 1/min": 6,
        "Tank Volume (Active) m3": 10,
        "Block Position m": 10,
    }
    for col, decimals in round_map.items():
        if col in df.columns and df[col].dtype != object:
            df[col] = df[col].round(decimals)

    # Verify header matches original
    original_cols = original_header.strip().split(",")
    assert list(df.columns) == original_cols, (
        f"Column mismatch!\n  Expected: {original_cols}\n  Got: {list(df.columns)}"
    )

    df.to_csv(path, index=False)
    print(f"  {len(df):,} rows written")


def main():
    if not INPUT.exists():
        print(f"ERROR: Input file not found: {INPUT}", file=sys.stderr)
        sys.exit(1)

    # Preserve original header for verification
    with open(INPUT) as f:
        original_header = f.readline()

    df = load(INPUT)
    to_numeric(df)

    # Pre-compute tank volume offset from original non-sparse data
    # (before forward-fill propagates values into sparse rows)
    tv_col = "Tank Volume (Active) m3"
    tv_valid = df[tv_col].dropna()
    tv_offset = abs(tv_valid.median())
    print(f"\nTank volume offset (|median| of {len(tv_valid):,} original values): "
          f"+{tv_offset:.2f} m3")

    # Fix bad values BEFORE forward-fill so clean data propagates
    # into sparse rows instead of errors
    print("\nCleaning steps:")
    step2_interpolate_neg_hookload(df)
    step3_fix_extreme_torque(df)
    step4_clamp_mild_neg_torque(df)
    step7_fill_rig_mode(df)

    # Now forward-fill clean values into sparse rows
    step1_forward_fill_sparse(df)

    # Apply tank volume offset (after ffill so all rows get shifted)
    step5_fix_tank_volume(df, tv_offset)
    step6_fill_ecd(df)

    save(df, OUTPUT, original_header)

    # Summary stats
    print("\nPost-clean validation:")
    for col in SENSOR_COLS:
        if col in df.columns:
            na = df[col].isna().sum()
            neg = (df[col] < 0).sum() if df[col].dtype != object else 0
            if na > 0 or neg > 0:
                print(f"  {col}: {na:,} NaN, {neg:,} negative")

    ecd_col = "Equivalent Circulating Density g/cm3"
    ecd_filled = df[ecd_col].notna().sum()
    print(f"  ECD filled: {ecd_filled:,} / {len(df):,}")
    print("\nDone.")


if __name__ == "__main__":
    main()
