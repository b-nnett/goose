from whoop_protocol.schema import load_schema


def test_load_schema_has_enums_and_packets():
    s = load_schema()
    # enum_name returns the suffixed "NAME(value)" form (matches legacy _name);
    # the bare packet-type name comes from type_name() instead.
    assert s.enum_name("EventNumber", 9) == "WRIST_ON(9)"
    assert s.enum_name("CommandNumber", 26) == "GET_BATTERY_LEVEL(26)"
    assert s.type_name(40) == "REALTIME_DATA"
    assert s.packet_for_type(40)["post"] == "realtime_data"
    # type-47 HISTORICAL_DATA is its OWN packet (the 14-day biometric store), not an alias
    # of REALTIME_RAW_DATA. (The old alias encoded the now-corrected assumption that history
    # carried no distinct type-47 payload — see protocol-complete §0-bis.)
    assert s.packet_for_type(47)["post"] == "historical_data"
    assert s.type_name(47) == "HISTORICAL_DATA"


def test_enum_name_unknown_value():
    s = load_schema()
    assert s.enum_name("EventNumber", 250) == "0xFA(250)"


def test_envelope_present():
    s = load_schema()
    names = [f["name"] for f in s.envelope]
    assert names == ["SOF", "length", "crc8", "packet_type", "seq"]
