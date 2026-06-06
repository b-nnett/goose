"""Load + index the declarative decode schema."""
import json
import os
from functools import lru_cache

_SCHEMA_PATH = os.path.join(os.path.dirname(__file__), "schema", "whoop_protocol.json")
_SCHEMA_PATH_5 = os.path.join(os.path.dirname(__file__), "schema", "whoop_protocol_5.json")


class Schema:
    def __init__(self, raw: dict):
        self._raw = raw
        self.enums = raw["enums"]
        self.envelope = raw["envelope"]
        self.packets = raw["packets"]
        # index packet spec by numeric type, honoring aliases. Iterate in sorted-name
        # order and let the FIRST packet claim a type — deterministic when two packets
        # share a type (5.0 EVENT vs EVENT_BATTERY_LEVEL both type 48). "EVENT" sorts
        # before "EVENT_BATTERY_LEVEL", so the generic EVENT spec (whose `event`
        # post-hook branches on the event number, including BATTERY_LEVEL) wins — the
        # 4.0 single-EVENT pattern. Without this the dict-insertion order made the
        # winner non-deterministic and broke Python<->Swift parity for type-48 frames.
        self._by_type = {}
        for name in sorted(self.packets):
            spec = {**self.packets[name], "name": name}
            self._by_type.setdefault(spec["type"], spec)
            for alias in spec.get("aliases", []):
                self._by_type.setdefault(alias, spec)

    def enum_name(self, enum: str, value: int) -> str:
        # Suffixed "NAME(value)" form, matching the legacy whoop_fields `_name()`.
        # Used for event / resp_cmd / meta_type / cmd_name field values. For the bare
        # packet-type name (out["type_name"]) use type_name() instead.
        table = self.enums.get(enum, {})
        name = table.get(str(value))
        return f"{name}({value})" if name else f"0x{value:02X}({value})"

    def type_name(self, value: int) -> str:
        table = self.enums["PacketType"]
        return table.get(str(value), f"type{value}")

    def packet_for_type(self, value: int):
        return self._by_type.get(value)


@lru_cache(maxsize=1)
def load_schema() -> Schema:
    with open(_SCHEMA_PATH) as fh:
        return Schema(json.load(fh))


@lru_cache(maxsize=1)
def load_schema_5() -> Schema:
    """Load the WHOOP 5.0 (Maverick) decode schema. Mirror of load_schema() for the
    whoop_protocol_5.json variant; load_schema() (4.0) is kept intact alongside it."""
    with open(_SCHEMA_PATH_5) as fh:
        return Schema(json.load(fh))
