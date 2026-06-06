"""Whoop live dashboard backend: BLE (bonded) -> parse -> WebSocket -> browser.
Serves the static UI and accepts control commands from the page.
Run:  ./whoop-reader/.venv/bin/python dashboard/server.py
Open: http://127.0.0.1:8765
"""
import asyncio, json, struct, sys, time, os
from pathlib import Path
sys.path.insert(0, "whoomp/scripts")
from packet import WhoopPacket, PacketType, CommandNumber, MetadataType  # noqa
from bleak import BleakClient, BleakScanner  # noqa
from aiohttp import web, WSMsgType  # noqa
sys.path.insert(0, str(Path(__file__).parent))
from whoop_fields import parse_frame, CATEGORIES  # noqa

# Personal device UUID is gitignored (see re/device_config.py); fall back to env var.
sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "re"))
try:
    from device_config import DEVICE_UUID as ADDR  # noqa
except Exception:
    ADDR = os.environ.get("WHOOP_DEVICE_UUID", "00000000-0000-0000-0000-000000000000")
CMD_TO = "61080002-8d6d-82b8-614a-1c8cb0f8dcc6"
CMD_FROM = "61080003-8d6d-82b8-614a-1c8cb0f8dcc6"
EVENTS = "61080004-8d6d-82b8-614a-1c8cb0f8dcc6"
DATA = "61080005-8d6d-82b8-614a-1c8cb0f8dcc6"
STATIC = Path(__file__).parent / "static"

clients = set()
state = {"connected": False, "device": None, "battery": None, "fw": None, "bonded": False}
outq: asyncio.Queue = asyncio.Queue()
ble = {"client": None}
_buf = {"b": b"", "need": 0}


def emit(obj):
    try:
        outq.put_nowait(obj)
    except Exception:
        pass


def cb_simple(char):
    def cb(_, d):
        on_frame(char, bytes(d))
    return cb


def cb_data(_, d):
    # reassemble fragmented frames on the data char
    f = bytes(d)
    if _buf["need"] == 0:
        if f and f[0] == 0xAA and len(f) >= 3:
            total = struct.unpack("<H", f[1:3])[0] + 4
            if len(f) >= total:
                on_frame("data", f[:total])
            else:
                _buf["b"], _buf["need"] = f, total
    else:
        _buf["b"] += f
        if len(_buf["b"]) >= _buf["need"]:
            on_frame("data", _buf["b"][:_buf["need"]])
            _buf["b"], _buf["need"] = b"", 0


async def send_cmd(name, payload=b"\x00"):
    c = ble["client"]
    if not c:
        return
    cmd = CommandNumber[name]
    pkt = WhoopPacket(PacketType.COMMAND, 10, cmd, data=payload).framed_packet()
    await c.write_gatt_char(CMD_TO, pkt, response=True)
    emit({"kind": "log", "msg": f"→ {name} {payload.hex()}"})


async def run_historical():
    emit({"kind": "log", "msg": "historical offload starting…"})
    await send_cmd("SEND_HISTORICAL_DATA")
    # metadata acks handled here by polling the queue is hard; do a simple timed loop
    # We rely on on_frame emitting metadata; ack via a dedicated meta queue:
    chunks = 0
    end = time.time() + 30
    while time.time() < end:
        await asyncio.sleep(0.05)
        while META.qsize():
            m = META.get_nowait()
            if m == "complete":
                emit({"kind": "log", "msg": f"HISTORY_COMPLETE ({chunks} chunks)"}); return
            else:
                await send_cmd("HISTORICAL_DATA_RESULT", struct.pack("<BLL", 1, m, 0))
                chunks += 1
                end = time.time() + 15
    emit({"kind": "log", "msg": f"historical stopped ({chunks} chunks)"})


META: asyncio.Queue = asyncio.Queue()


def meta_hook(rec):
    if rec.get("type_name") == "METADATA":
        mt = rec.get("parsed", {})
        # find trim cursor / complete
        for f in rec.get("fields", []):
            if f["name"] == "meta_type" and "COMPLETE" in str(f["value"]):
                META.put_nowait("complete"); return
            if f["name"] == "trim_cursor":
                META.put_nowait(f["value"])


def on_frame(char, frame):
    rec = parse_frame(frame)
    rec["char"] = char; rec["ts"] = time.time()
    p = rec.get("parsed", {})
    if "battery_pct" in p: state["battery"] = p["battery_pct"]
    if "fw_harvard" in p: state["fw"] = p["fw_harvard"]
    meta_hook(rec)
    emit({"kind": "packet", "packet": rec, "state": state})


async def ble_loop():
    while True:
        try:
            dev = await BleakScanner.find_device_by_address(ADDR, timeout=15.0)
            if not dev:
                emit({"kind": "log", "msg": "device not found, retry 5s"}); state["connected"] = False
                emit({"kind": "state", "state": state}); await asyncio.sleep(5); continue
            async with BleakClient(dev) as c:
                ble["client"] = c
                state.update(connected=True, device=dev.name)
                emit({"kind": "log", "msg": f"connected {dev.name}"})
                emit({"kind": "state", "state": state})
                await c.start_notify(CMD_FROM, cb_simple("cmd_from"))
                await c.start_notify(EVENTS, cb_simple("events"))
                await c.start_notify(DATA, cb_data)
                await send_cmd("GET_BATTERY_LEVEL")  # confirmed write -> bond
                state["bonded"] = True
                await asyncio.sleep(0.5)
                await send_cmd("REPORT_VERSION_INFO")
                await send_cmd("TOGGLE_REALTIME_HR", b"\x01")
                # keepalive + battery poll
                while c.is_connected:
                    await asyncio.sleep(20)
                    await send_cmd("GET_BATTERY_LEVEL")
        except Exception as e:
            emit({"kind": "log", "msg": f"ble error: {e}"})
        ble["client"] = None
        state["connected"] = False
        emit({"kind": "state", "state": state})
        await asyncio.sleep(3)


async def broadcaster():
    while True:
        obj = await outq.get()
        dead = []
        for ws in clients:
            try:
                await ws.send_json(obj)
            except Exception:
                dead.append(ws)
        for ws in dead:
            clients.discard(ws)


async def ws_handler(request):
    ws = web.WebSocketResponse()
    await ws.prepare(request)
    clients.add(ws)
    await ws.send_json({"kind": "hello", "state": state, "categories": CATEGORIES})
    try:
        async for msg in ws:
            if msg.type == WSMsgType.TEXT:
                d = json.loads(msg.data)
                a = d.get("action")
                try:
                    if a == "start_realtime": await send_cmd("TOGGLE_REALTIME_HR", b"\x01")
                    elif a == "stop_realtime": await send_cmd("TOGGLE_REALTIME_HR", b"\x00")
                    elif a == "start_raw":
                        await send_cmd("ENABLE_OPTICAL_DATA", b"\x01")
                        await send_cmd("TOGGLE_IMU_MODE", b"\x01")
                        await send_cmd("START_RAW_DATA", b"\x01")
                    elif a == "stop_raw": await send_cmd("STOP_RAW_DATA", b"\x00")
                    elif a == "historical": asyncio.create_task(run_historical())
                    elif a == "cmd": await send_cmd(d["name"], bytes.fromhex(d.get("payload", "00")))
                except Exception as e:
                    emit({"kind": "log", "msg": f"cmd error: {e}"})
    finally:
        clients.discard(ws)
    return ws


async def index(request):
    return web.FileResponse(STATIC / "index.html")


def make_app():
    app = web.Application()
    app.router.add_get("/", index)
    app.router.add_get("/ws", ws_handler)
    app.router.add_static("/static", STATIC)
    return app


async def main():
    app = make_app()
    runner = web.AppRunner(app)
    await runner.setup()
    site = web.TCPSite(runner, "127.0.0.1", 8765)
    await site.start()
    print("Dashboard at http://127.0.0.1:8765", flush=True)
    asyncio.create_task(broadcaster())
    asyncio.create_task(ble_loop())
    while True:
        await asyncio.sleep(3600)


if __name__ == "__main__":
    asyncio.run(main())
