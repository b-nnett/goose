"""
client.py — Read-only WHOOP Developer API v2 client.

Authenticates via OAuth2 authorization-code flow with rotating refresh tokens,
paginates collection endpoints, and normalizes responses into GroundTruthDay /
GroundTruthWorkout records.

See docs/research/05-whoop-api.md for the full spec (auth URLs, field names,
pagination semantics, join/alignment rules).

Usage
-----
    from app.whoop_api.client import WhoopClient
    from datetime import datetime, timezone

    client = WhoopClient(
        client_id="...",
        client_secret="...",
        refresh_token="...",
        on_token_refresh=lambda new_rt: save_somewhere(new_rt),
    )
    days = client.ground_truth_days(
        start=datetime(2026, 5, 1, tzinfo=timezone.utc),
        end=datetime(2026, 5, 26, tzinfo=timezone.utc),
    )

All secrets must come from environment variables — never hard-code or log them.
"""

from __future__ import annotations

import logging
import secrets
import time
import urllib.parse
from datetime import date, datetime, timedelta, timezone, tzinfo
from typing import Callable, Generator, Iterator

import httpx

from .models import GroundTruthDay, GroundTruthWorkout

log = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# Constants (from docs/research/05-whoop-api.md §1.2)
# ---------------------------------------------------------------------------

AUTH_URL  = "https://api.prod.whoop.com/oauth/oauth2/auth"
TOKEN_URL = "https://api.prod.whoop.com/oauth/oauth2/token"
API_BASE  = "https://api.prod.whoop.com/developer"

REQUIRED_SCOPES = (
    "read:recovery read:sleep read:cycles read:workout "
    "read:body_measurement read:profile offline"
)

# Pagination: WHOOP enforces limit <= 25 (§2.2)
_PAGE_LIMIT = 25

# 429 back-off: simple exponential, capped at 60 s
_BACKOFF_BASE_S = 2.0
_BACKOFF_MAX_S  = 60.0


# ---------------------------------------------------------------------------
# Public: one-time auth-code helper (first-time setup only)
# ---------------------------------------------------------------------------

def build_auth_url(client_id: str, redirect_uri: str,
                   scope: str = REQUIRED_SCOPES,
                   state: str | None = None) -> str:
    """Return the browser URL the user must visit to authorize.

    The state param must be >= 8 chars (WHOOP requirement).  A random one is
    generated when not supplied.  The caller is responsible for verifying the
    state echoed back in the redirect.
    """
    if state is None:
        state = secrets.token_urlsafe(12)
    params = {
        "response_type": "code",
        "client_id": client_id,
        "redirect_uri": redirect_uri,
        "scope": scope,
        "state": state,
    }
    return AUTH_URL + "?" + urllib.parse.urlencode(params)


def exchange_code(code: str, client_id: str, client_secret: str,
                  redirect_uri: str,
                  http_client: httpx.Client | None = None) -> dict:
    """Exchange a one-time auth code for the first access + refresh token pair.

    Returns the raw token response dict:
        {"access_token": "...", "refresh_token": "...",
         "expires_in": 3600, "token_type": "bearer", "scope": "..."}

    Raises httpx.HTTPStatusError on non-2xx.
    """
    client = http_client or httpx.Client()
    resp = client.post(TOKEN_URL, data={
        "grant_type":    "authorization_code",
        "code":          code,
        "client_id":     client_id,
        "client_secret": client_secret,
        "redirect_uri":  redirect_uri,
    })
    resp.raise_for_status()
    return resp.json()


# ---------------------------------------------------------------------------
# WhoopClient
# ---------------------------------------------------------------------------

class WhoopClient:
    """Read-only WHOOP v2 API client with rotating-refresh-token OAuth2.

    Parameters
    ----------
    client_id, client_secret:
        From the WHOOP Developer Dashboard app.
    refresh_token:
        Stored from the initial auth-code exchange (or a previous rotation).
    on_token_refresh:
        Called with the NEW refresh token string every time WHOOP rotates it.
        Use this to persist the new token so the next run doesn't have to
        re-authorize from scratch.  If None, rotated tokens are held in-memory
        only for the lifetime of this object.
    http_client:
        Optional injected httpx.Client (used in tests to pass a MockTransport).
    """

    def __init__(
        self,
        client_id: str,
        client_secret: str,
        refresh_token: str,
        on_token_refresh: Callable[[str], None] | None = None,
        http_client: httpx.Client | None = None,
    ) -> None:
        self._client_id     = client_id
        self._client_secret = client_secret
        self._refresh_token = refresh_token
        self._on_token_refresh = on_token_refresh

        # access-token state
        self._access_token: str | None = None
        self._token_expires_at: float  = 0.0   # epoch seconds

        self._http = http_client or httpx.Client()

    # ------------------------------------------------------------------
    # Token management
    # ------------------------------------------------------------------

    def _refresh(self) -> None:
        """Rotate the refresh token and update the access token."""
        resp = self._http.post(TOKEN_URL, data={
            "grant_type":    "refresh_token",
            "refresh_token": self._refresh_token,
            "client_id":     self._client_id,
            "client_secret": self._client_secret,
            "scope":         "offline",
        })
        resp.raise_for_status()
        data = resp.json()

        self._access_token     = data["access_token"]
        self._refresh_token    = data["refresh_token"]   # WHOOP rotates it
        # Leave a 30 s buffer before considering the token expired
        self._token_expires_at = time.monotonic() + data.get("expires_in", 3600) - 30

        if self._on_token_refresh:
            self._on_token_refresh(self._refresh_token)
        log.debug("Token refreshed; new refresh token persisted via callback.")

    def _ensure_token(self) -> str:
        """Return a valid access token, refreshing if expired or missing."""
        if self._access_token is None or time.monotonic() >= self._token_expires_at:
            self._refresh()
        assert self._access_token is not None
        return self._access_token

    # ------------------------------------------------------------------
    # Low-level HTTP
    # ------------------------------------------------------------------

    def _get(self, path: str, params: dict | None = None) -> dict:
        """Authorized GET; retries once after a 401 (token expiry) and backs off on 429."""
        url = API_BASE + path
        for attempt in range(2):
            token = self._ensure_token()
            resp  = self._http.get(url, headers={"Authorization": f"Bearer {token}"},
                                   params=params or {})
            if resp.status_code == 401 and attempt == 0:
                # Force a refresh and retry exactly once
                self._access_token = None
                continue
            if resp.status_code == 429:
                retry_after = float(resp.headers.get("Retry-After", _BACKOFF_BASE_S))
                wait = min(retry_after, _BACKOFF_MAX_S)
                log.warning("Rate-limited; waiting %.1f s", wait)
                time.sleep(wait)
                continue
            resp.raise_for_status()
            return resp.json()
        # Should not reach here in normal operation
        raise RuntimeError(f"Failed GET {path} after retries")

    # ------------------------------------------------------------------
    # Pagination
    # ------------------------------------------------------------------

    def _paginate(
        self,
        path: str,
        start: datetime,
        end: datetime,
        limit: int = _PAGE_LIMIT,
    ) -> Iterator[dict]:
        """Yield every record across pages for a collection endpoint.

        Threads nextToken ← next_token until the cursor is absent/null.
        Note the asymmetry: request param is `nextToken`, response field is
        `next_token` (§2.2 of the research doc).
        """
        params: dict = {
            "limit": min(limit, _PAGE_LIMIT),
            "start": start.isoformat(),
            "end":   end.isoformat(),
        }
        seen_tokens: set[str] = set()
        while True:
            page = self._get(path, params)
            for record in page.get("records", []):
                yield record
            next_token = page.get("next_token")
            if not next_token:
                break
            if next_token in seen_tokens:
                log.warning(
                    "_paginate: next_token %r repeated on %s — stopping to avoid infinite loop",
                    next_token, path,
                )
                break
            seen_tokens.add(next_token)
            params = {"limit": params["limit"], "nextToken": next_token}

    # ------------------------------------------------------------------
    # Raw iterators
    # ------------------------------------------------------------------

    def iter_cycles(self, start: datetime, end: datetime) -> Iterator[dict]:
        yield from self._paginate("/v2/cycle", start, end)

    def iter_recoveries(self, start: datetime, end: datetime) -> Iterator[dict]:
        yield from self._paginate("/v2/recovery", start, end)

    def iter_sleeps(self, start: datetime, end: datetime) -> Iterator[dict]:
        yield from self._paginate("/v2/activity/sleep", start, end)

    def iter_workouts(self, start: datetime, end: datetime) -> Iterator[dict]:
        yield from self._paginate("/v2/activity/workout", start, end)

    # ------------------------------------------------------------------
    # Normalization helpers
    # ------------------------------------------------------------------

    @staticmethod
    def _local_date(ts: str, tz_offset: str) -> date:
        """Parse an ISO-8601 timestamp and shift it by the WHOOP tz offset.

        tz_offset is e.g. "-05:00" or "+02:00".
        """
        # Parse the timestamp (may already carry tz info)
        dt_utc = datetime.fromisoformat(ts.replace("Z", "+00:00"))
        # Parse the offset
        sign  = 1 if tz_offset[0] == "+" else -1
        parts = tz_offset.lstrip("+-").split(":")
        delta = timedelta(hours=int(parts[0]), minutes=int(parts[1] if len(parts) > 1 else "0"))
        local_dt = dt_utc + sign * delta
        return local_dt.date()

    @staticmethod
    def _day_from_cycle(cycle: dict) -> date:
        return WhoopClient._local_date(cycle["start"], cycle.get("timezone_offset", "+00:00"))

    # ------------------------------------------------------------------
    # Normalized ground truth
    # ------------------------------------------------------------------

    def ground_truth_days(
        self,
        start: datetime,
        end: datetime,
    ) -> dict[date, GroundTruthDay]:
        """Pull cycles + recoveries + sleeps over [start, end].

        Joins by cycle_id and returns one GroundTruthDay per local calendar date.
        Skips entries where score_state != 'SCORED'; leaves those fields None.
        Skips nap sleeps (nap == True).
        """
        # ---- gather raw data ----
        cycles     = list(self.iter_cycles(start, end))
        recoveries = list(self.iter_recoveries(start, end))
        sleeps     = list(self.iter_sleeps(start, end))

        # index by cycle_id
        rec_by_cycle: dict[int, dict] = {}
        for r in recoveries:
            if r.get("score_state") == "SCORED":
                rec_by_cycle[r["cycle_id"]] = r

        # For sleeps: index by cycle_id, prefer non-nap + longest in_bed
        sleep_by_cycle: dict[int, dict] = {}
        for s in sleeps:
            if s.get("nap", False):
                continue
            if s.get("score_state") != "SCORED":
                continue
            cid = s.get("cycle_id")
            if cid is None:
                continue
            existing = sleep_by_cycle.get(cid)
            if existing is None:
                sleep_by_cycle[cid] = s
            else:
                # prefer the one with more in-bed time
                new_ib  = (s.get("score") or {}).get("stage_summary", {}).get("total_in_bed_time_milli", 0)
                prev_ib = (existing.get("score") or {}).get("stage_summary", {}).get("total_in_bed_time_milli", 0)
                if new_ib > prev_ib:
                    sleep_by_cycle[cid] = s

        # ---- build GroundTruthDay per cycle ----
        days: dict[date, GroundTruthDay] = {}
        for cycle in cycles:
            cid        = cycle["id"]
            local_day  = self._day_from_cycle(cycle)
            scored     = cycle.get("score_state") == "SCORED"
            cscore     = (cycle.get("score") or {}) if scored else {}

            recovery   = rec_by_cycle.get(cid)
            rscore     = (recovery.get("score") or {}) if recovery else {}

            sleep      = sleep_by_cycle.get(cid)
            ss_score   = (sleep.get("score") or {}) if sleep else {}
            stage      = ss_score.get("stage_summary") or {}

            gtd = GroundTruthDay(
                day      = local_day,
                cycle_id = cid,

                # cycle
                day_strain = cscore.get("strain"),
                kilojoule  = cscore.get("kilojoule"),
                avg_hr     = cscore.get("average_heart_rate"),
                max_hr     = cscore.get("max_heart_rate"),

                # recovery
                recovery_score    = rscore.get("recovery_score"),
                resting_hr        = rscore.get("resting_heart_rate"),
                hrv_rmssd_milli   = rscore.get("hrv_rmssd_milli"),
                spo2_percentage   = rscore.get("spo2_percentage"),
                skin_temp_celsius = rscore.get("skin_temp_celsius"),
                user_calibrating  = bool(rscore.get("user_calibrating", False)),

                # sleep
                sleep_id               = sleep["id"]  if sleep else None,
                sleep_start            = sleep["start"] if sleep else None,
                sleep_end              = sleep["end"]   if sleep else None,
                in_bed_milli           = stage.get("total_in_bed_time_milli"),
                awake_milli            = stage.get("total_awake_time_milli"),
                no_data_milli          = stage.get("total_no_data_time_milli"),
                light_milli            = stage.get("total_light_sleep_time_milli"),
                sws_deep_milli         = stage.get("total_slow_wave_sleep_time_milli"),
                rem_milli              = stage.get("total_rem_sleep_time_milli"),
                sleep_cycle_count      = stage.get("sleep_cycle_count"),
                disturbance_count      = stage.get("disturbance_count"),
                respiratory_rate       = ss_score.get("respiratory_rate"),
                sleep_performance_pct  = ss_score.get("sleep_performance_percentage"),
                sleep_efficiency_pct   = ss_score.get("sleep_efficiency_percentage"),
                sleep_consistency_pct  = ss_score.get("sleep_consistency_percentage"),
            )
            if rscore.get("user_calibrating"):
                log.debug(
                    "Cycle %s (%s): user_calibrating=True — recovery score is not stable",
                    cid, local_day,
                )

            # First-seen-wins: WHOOP returns cycles in descending order (most
            # recent first), so the first time we encounter a date it carries
            # the most-recent cycle for that day.  Subsequent cycles for the
            # same local date are older and should be ignored.
            if local_day not in days:
                days[local_day] = gtd

        return days

    def ground_truth_workouts(
        self,
        start: datetime,
        end: datetime,
    ) -> list[GroundTruthWorkout]:
        """Pull workouts over [start, end] and normalize into GroundTruthWorkout records."""
        workouts = []
        for w in self.iter_workouts(start, end):
            if w.get("score_state") != "SCORED":
                wscore = {}
            else:
                wscore = (w.get("score") or {})

            workouts.append(GroundTruthWorkout(
                workout_id    = w["id"],
                start         = w["start"],
                end           = w["end"],
                sport_name    = w.get("sport_name"),
                sport_id      = w.get("sport_id"),
                strain        = wscore.get("strain"),
                avg_hr        = wscore.get("average_heart_rate"),
                max_hr        = wscore.get("max_heart_rate"),
                kilojoule     = wscore.get("kilojoule"),
                distance_meter= wscore.get("distance_meter"),
            ))
        return workouts
