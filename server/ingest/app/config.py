"""Ingest service configuration from environment."""
import os
from dataclasses import dataclass


@dataclass(frozen=True)
class Config:
    api_key: str
    db_dsn: str
    raw_root: str


def load_config() -> Config:
    api_key = os.environ.get("GOOSE_API_KEY")
    db_dsn = os.environ.get("GOOSE_DB_DSN")
    raw_root = os.environ.get("GOOSE_RAW_ROOT", "/data/raw")
    if not api_key:
        raise RuntimeError("GOOSE_API_KEY is required")
    if not db_dsn:
        raise RuntimeError("GOOSE_DB_DSN is required")
    return Config(api_key=api_key, db_dsn=db_dsn, raw_root=raw_root)
