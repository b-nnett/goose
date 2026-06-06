"""Test fixtures: a throwaway TimescaleDB container + a configured FastAPI TestClient.
Integration tests skip automatically if Docker is unavailable."""
import os
import shutil
import subprocess
import time
import uuid

import pytest

IMAGE = "timescale/timescaledb:2.17.2-pg16"
DB_NAME = "whoop_test"
DB_USER = "whoop"
DB_PASSWORD = "testpw"


def _docker_available():
    if shutil.which("docker") is None:
        return False
    return subprocess.run(["docker", "info"], capture_output=True).returncode == 0


requires_docker = pytest.mark.skipif(not _docker_available(), reason="docker not available")


@pytest.fixture(scope="session")
def timescale_dsn():
    """Start a throwaway TimescaleDB container, apply init.sql, yield a DSN, tear down."""
    if not _docker_available():
        pytest.skip("docker not available")
    import psycopg

    name = f"whoop-test-db-{uuid.uuid4().hex[:8]}"
    # publish container 5432 on an ephemeral host port (-P picks a free one)
    subprocess.run([
        "docker", "run", "-d", "--rm", "--name", name,
        "-e", f"POSTGRES_DB={DB_NAME}", "-e", f"POSTGRES_USER={DB_USER}",
        "-e", f"POSTGRES_PASSWORD={DB_PASSWORD}", "-P", IMAGE,
    ], check=True, capture_output=True)
    try:
        port = subprocess.run(
            ["docker", "port", name, "5432/tcp"], check=True, capture_output=True, text=True
        ).stdout.strip().rsplit(":", 1)[1]
        dsn = f"postgresql://{DB_USER}:{DB_PASSWORD}@127.0.0.1:{port}/{DB_NAME}"
        # wait for readiness
        deadline = time.time() + 60
        while True:
            try:
                with psycopg.connect(dsn, connect_timeout=3) as c:
                    c.execute("SELECT 1")
                break
            except Exception:
                if time.time() > deadline:
                    raise
                time.sleep(1)
        # apply schema
        init_sql = os.path.join(os.path.dirname(__file__), "..", "..", "db", "init.sql")
        with open(init_sql) as fh, psycopg.connect(dsn, autocommit=True) as c:
            c.execute(fh.read())
        yield dsn
    finally:
        subprocess.run(["docker", "rm", "-f", name], capture_output=True)


@pytest.fixture
def clean_db(timescale_dsn):
    """Truncate all data tables before each test for isolation."""
    import psycopg
    with psycopg.connect(timescale_dsn, autocommit=True) as c:
        c.execute("TRUNCATE devices, raw_batches, hr_samples, rr_intervals, events, battery, "
                  "spo2_samples, skin_temp_samples, resp_samples, gravity_samples, "
                  "sleep_sessions, exercise_sessions, daily_metrics, profile CASCADE")
    return timescale_dsn
