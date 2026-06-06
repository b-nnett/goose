"""
import_csv_to_grdb.py — Import WHOOP data export CSV into the iOS GRDB SQLite database.

Usage:
    python import_csv_to_grdb.py \
        --zip /path/to/my_whoop_data_2026_06_01.zip \
        --db  /path/to/OpenWhoop.sqlite \
        --device-id <device_id>

The script parses the Portuguese (pt-PT) or English WHOOP CSV export and upserts
all GroundTruthDay records into the dailyMetric table that the iOS app reads via WhoopStore.

Field mapping (CSV -> dailyMetric):
    recovery_score %      -> recovery   (divides by 100.0 to get 0.0-1.0)
    hrv (ms)              -> avgHrv
    resting_hr (bpm)      -> restingHr
    spo2 %                -> spo2Pct
    skin_temp (celsius)   -> skinTempDevC
    resp_rate (rpm)       -> respRateBpm
    sleep_perf %          -> sleepPerformance
    sleep_need (min)      -> sleepNeededMin
    total_sleep (min)     -> totalSleepMin
    deep (min)            -> deepMin
    rem (min)             -> remMin
    light (min)           -> lightMin
    awake (min)           -> disturbances (proxy: no direct field)
    strain                -> strain
"""
from __future__ import annotations

import argparse
import csv
import re
import sqlite3
import sys
import zipfile
from datetime import datetime
from pathlib import Path


# ---------------------------------------------------------------------------
# Header normalization (handles both English and Portuguese)
# ---------------------------------------------------------------------------

import unicodedata

def _norm(h: str) -> str:
    # Decompose unicode → strip diacritics → re-encode as ASCII
    s = unicodedata.normalize('NFD', h.lower().strip())
    s = ''.join(c for c in s if unicodedata.category(c) != 'Mn')
    s = s.replace('%', 'pct')
    s = re.sub(r'[^a-z0-9]+', '_', s)
    return s.strip('_')


# Header aliases: normalized -> internal key
_ALIASES: dict[str, str] = {
    # Physiological cycles / recovery
    'hora_de_inicio_do_ciclo': 'cycle_start',
    'cycle_start': 'cycle_start',
    'hora_de_fim_do_ciclo': 'cycle_end',
    'pontuacao_de_recuperacao_pct': 'recovery',
    'recovery_score_pct': 'recovery',
    'frequencia_cardiaca_em_repouso_bpm': 'resting_hr',
    'resting_heart_rate_bpm': 'resting_hr',
    'variabilidade_da_frequencia_cardiaca_ms': 'hrv_ms',
    'heart_rate_variability_ms': 'hrv_ms',
    'temp_da_pele_celsius': 'skin_temp',
    'skin_temp_celsius': 'skin_temp',
    'pct_de_oxigenio_no_sangue': 'spo2',
    'blood_oxygen_level_pct': 'spo2',
    'esforco_diario': 'strain',
    'day_strain': 'strain',
    # Sleep
    'desempenho_do_sono_pct': 'sleep_perf',
    'sleep_performance_score_pct': 'sleep_perf',
    'frequencia_respiratoria_rpm': 'resp_rpm',
    'respiratory_rate_rpm': 'resp_rpm',
    'duracao_do_sono_min': 'total_sleep',
    'asleep_duration_min': 'total_sleep',
    'duracao_na_cama_min': 'in_bed',
    'in_bed_duration_min': 'in_bed',
    'duracao_do_sono_leve_min': 'light',
    'light_sleep_duration_min': 'light',
    'duracao_profundo_sono_min': 'deep',
    'sws_duration_min': 'deep',
    'duracao_rem_min': 'rem',
    'rem_duration_min': 'rem',
    'necessidade_de_sono_min': 'sleep_need',
    'sleep_need_min': 'sleep_need',
}


def _parse_float(v: str | None) -> float | None:
    if not v:
        return None
    try:
        return float(v)
    except ValueError:
        return None


def _parse_int(v: str | None) -> int | None:
    f = _parse_float(v)
    return None if f is None else int(round(f))


def _cycle_date(cycle_start: str | None, tz_offset: str | None) -> str | None:
    """Return YYYY-MM-DD local date for the cycle."""
    if not cycle_start:
        return None
    try:
        dt = datetime.fromisoformat(cycle_start)
        return dt.date().isoformat()
    except ValueError:
        return None


# ---------------------------------------------------------------------------
# CSV parsing
# ---------------------------------------------------------------------------

def _find_csv(extract_dir: Path, candidates: list[str]) -> Path | None:
    for p in extract_dir.rglob('*.csv'):
        if any(c in p.name.lower() for c in candidates):
            return p
    return None


def parse_cycles(path: Path) -> list[dict]:
    """Parse cycles CSV into normalised dicts."""
    rows = []
    with open(path, encoding='utf-8-sig') as f:
        reader = csv.DictReader(f)
        for raw in reader:
            row: dict[str, object] = {}
            for k, v in raw.items():
                key = _ALIASES.get(_norm(k), _norm(k))
                row[key] = v.strip() if v else None
            # Derive day
            row['day'] = _cycle_date(row.get('cycle_start'), row.get('fuso_horario_do_ciclo'))
            if row['day']:
                rows.append(row)
    return rows


# ---------------------------------------------------------------------------
# GRDB upsert
# ---------------------------------------------------------------------------

UPSERT_SQL = """
INSERT INTO dailyMetric
    (deviceId, day, recovery, restingHr, avgHrv,
     spo2Pct, skinTempDevC, respRateBpm,
     sleepPerformance, sleepNeededMin,
     totalSleepMin, deepMin, remMin, lightMin, strain)
VALUES
    (?, ?, ?, ?, ?,
     ?, ?, ?,
     ?, ?,
     ?, ?, ?, ?, ?)
ON CONFLICT(deviceId, day) DO UPDATE SET
    recovery        = COALESCE(excluded.recovery, recovery),
    restingHr       = COALESCE(excluded.restingHr, restingHr),
    avgHrv          = COALESCE(excluded.avgHrv, avgHrv),
    spo2Pct         = COALESCE(excluded.spo2Pct, spo2Pct),
    skinTempDevC    = COALESCE(excluded.skinTempDevC, skinTempDevC),
    respRateBpm     = COALESCE(excluded.respRateBpm, respRateBpm),
    sleepPerformance= COALESCE(excluded.sleepPerformance, sleepPerformance),
    sleepNeededMin  = COALESCE(excluded.sleepNeededMin, sleepNeededMin),
    totalSleepMin   = COALESCE(excluded.totalSleepMin, totalSleepMin),
    deepMin         = COALESCE(excluded.deepMin, deepMin),
    remMin          = COALESCE(excluded.remMin, remMin),
    lightMin        = COALESCE(excluded.lightMin, lightMin),
    strain          = COALESCE(excluded.strain, strain)
"""


def upsert_days(db_path: Path, device_id: str, rows: list[dict], dry_run: bool = False) -> int:
    conn = sqlite3.connect(db_path)
    cur = conn.cursor()
    inserted = 0
    for r in rows:
        recovery_raw = _parse_float(r.get('recovery'))
        params = (
            device_id,
            r['day'],
            recovery_raw / 100.0 if recovery_raw is not None else None,  # 0.0-1.0
            _parse_int(r.get('resting_hr')),
            _parse_float(r.get('hrv_ms')),
            _parse_float(r.get('spo2')),
            _parse_float(r.get('skin_temp')),
            _parse_float(r.get('resp_rpm')),
            _parse_float(r.get('sleep_perf')),  # already 0-100
            _parse_float(r.get('sleep_need')),
            _parse_float(r.get('total_sleep')),
            _parse_float(r.get('deep')),
            _parse_float(r.get('rem')),
            _parse_float(r.get('light')),
            _parse_float(r.get('strain')),
        )
        if dry_run:
            print(f"  DRY RUN: day={r['day']} recovery={params[2]} hrv={params[4]}")
        else:
            cur.execute(UPSERT_SQL, params)
        inserted += 1
    if not dry_run:
        conn.commit()
    conn.close()
    return inserted


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> None:
    p = argparse.ArgumentParser(description='Import WHOOP CSV export into iOS GRDB SQLite')
    p.add_argument('--zip', required=True, help='Path to my_whoop_data_*.zip')
    p.add_argument('--db',  required=True, help='Path to the iOS OpenWhoop.sqlite file')
    p.add_argument('--device-id', required=True, help='WHOOP device UUID (see iOS app settings)')
    p.add_argument('--dry-run', action='store_true', help='Parse and print without writing')
    args = p.parse_args()

    zip_path = Path(args.zip)
    db_path  = Path(args.db)

    if not zip_path.exists():
        print(f'ERROR: zip not found: {zip_path}', file=sys.stderr); sys.exit(1)
    if not args.dry_run and not db_path.exists():
        print(f'ERROR: DB not found: {db_path}', file=sys.stderr); sys.exit(1)

    extract_dir = zip_path.parent / zip_path.stem
    extract_dir.mkdir(exist_ok=True)

    print(f'Extracting {zip_path.name} ...', file=sys.stderr)
    with zipfile.ZipFile(zip_path) as zf:
        zf.extractall(extract_dir)

    cycles_path = _find_csv(extract_dir, ['ciclo', 'physiological'])
    if not cycles_path:
        print('ERROR: cycles CSV not found in zip', file=sys.stderr); sys.exit(1)

    print(f'Parsing {cycles_path.name} ...', file=sys.stderr)
    rows = parse_cycles(cycles_path)
    print(f'  {len(rows)} days ({rows[-1]["day"]} → {rows[0]["day"]})', file=sys.stderr)

    n = upsert_days(db_path, args.device_id, rows, dry_run=args.dry_run)
    action = 'Would insert/update' if args.dry_run else 'Inserted/updated'
    print(f'{action} {n} days in dailyMetric.', file=sys.stderr)


if __name__ == '__main__':
    main()
