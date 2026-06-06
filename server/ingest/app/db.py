"""Database connection + idempotent schema bootstrap."""
import os

import psycopg

_SCHEMA_PATH = os.path.join(os.path.dirname(__file__), "..", "..", "db", "init.sql")
# In the container the file is at /app/../db? No — bundle the schema next to the app.
# We resolve relative to this file; in the image init.sql is copied to /app/db/init.sql
# (see Dockerfile change in this task's Step 5).
_SCHEMA_PATHS = [
    os.path.join(os.path.dirname(__file__), "db", "init.sql"),   # in-image: /app/app/db? no
    os.path.join(os.path.dirname(__file__), "..", "db", "init.sql"),  # /app/db/init.sql
    os.path.join(os.path.dirname(__file__), "..", "..", "db", "init.sql"),  # repo layout
]


def _schema_sql() -> str:
    for p in _SCHEMA_PATHS:
        if os.path.exists(p):
            with open(p) as fh:
                return fh.read()
    raise FileNotFoundError("init.sql not found in expected locations")


def connect(dsn: str) -> psycopg.Connection:
    return psycopg.connect(dsn)


def bootstrap_schema(dsn: str) -> None:
    """Apply init.sql idempotently (CREATE ... IF NOT EXISTS / create_hypertable if_not_exists)."""
    with psycopg.connect(dsn, autocommit=True) as conn:
        conn.execute(_schema_sql())
