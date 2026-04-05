#!/usr/bin/env python3
"""Decomposed exp145 — isolate each phase to find the exact crash vector.

Each phase runs as a separate ember RPC with fsync tracing. If the system
crashes, the trace file survives and shows exactly which phase killed it.

Run: python3 scripts/exp145_decomposed.py [phase]
  phase = all | a | b_safe | b_pramin_1page | b_pramin_all | b_bind | c | skip_to_c
"""

import socket, json, sys, os, time, struct

BDF = os.environ.get("CORALREEF_VFIO_BDF", "0000:03:00.0")
SOCKET = "/run/coralreef/ember.sock"
TRACE = "/var/lib/coralreef/traces/exp145_decomposed.log"

SEC2_BASE = 0x087000

def trace(msg):
    ts = int(time.time() * 1000)
    line = f"[{ts}] {msg}\n"
    sys.stderr.write(f"  TRACE: {msg}\n")
    sys.stderr.flush()
    with open(TRACE, "a") as f:
        f.write(line)
        f.flush()
        os.fsync(f.fileno())

def ember_rpc(method, params, timeout=10):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(SOCKET)
    s.settimeout(timeout)
    req = json.dumps({"jsonrpc": "2.0", "method": method, "params": params, "id": 1})
    s.sendall((req + "\n").encode())
    buf = b""
    while b"\n" not in buf and len(buf) < 4*1024*1024:
        chunk = s.recv(65536)
        if not chunk:
            break
        buf += chunk
    s.close()
    resp = json.loads(buf)
    if "error" in resp:
        raise RuntimeError(f"{method}: {resp['error'].get('message', 'unknown')}")
    return resp.get("result", {})

def mmio_read(offset, label=""):
    r = ember_rpc("ember.mmio.read", {"bdf": BDF, "offset": offset})
    val = r.get("value", 0xDEAD)
    trace(f"READ {label}@{offset:#x} = {val:#010x}")
    return val

def mmio_write(offset, value, label=""):
    trace(f"WRITE {label}@{offset:#x} = {value:#010x}")
    ember_rpc("ember.mmio.write", {"bdf": BDF, "offset": offset, "value": value})

def pramin_write(vram_addr, data):
    """Write raw bytes to VRAM via ember.pramin.write."""
    import base64
    encoded = base64.b64encode(data).decode()
    r = ember_rpc("ember.pramin.write",
                  {"bdf": BDF, "vram_addr": vram_addr, "data_b64": encoded},
                  timeout=30)
    return r.get("bytes_written", 0)

def pramin_read(vram_addr, length):
    """Read raw bytes from VRAM via ember.pramin.read."""
    import base64
    r = ember_rpc("ember.pramin.read",
                  {"bdf": BDF, "vram_addr": vram_addr, "length": length},
                  timeout=30)
    return base64.b64decode(r.get("data_b64", ""))

# ─── Phase A: Read GPU state ────────────────────────────────────────
def phase_a():
    trace("PHASE_A: reading GPU state")
    boot0 = mmio_read(0x0, "BOOT0")
    pmc = mmio_read(0x200, "PMC_ENABLE")
    sec2_cpuctl = mmio_read(SEC2_BASE + 0x100, "SEC2_CPUCTL")
    sec2_sctl = mmio_read(SEC2_BASE + 0x240, "SEC2_SCTL")
    trace(f"PHASE_A: BOOT0={boot0:#010x} PMC={pmc:#010x} CPUCTL={sec2_cpuctl:#010x} SCTL={sec2_sctl:#010x}")
    if boot0 == 0xFFFFFFFF or boot0 == 0xDEAD:
        trace("PHASE_A: BOOT0 dead — aborting")
        return False
    trace("PHASE_A: OK")
    return True

# ─── Phase PRAMIN_PROBE: read-only PRAMIN test (no writes) ──────────
def phase_pramin_probe():
    """Read from PRAMIN to check if VRAM is alive. No writes — safest possible test."""
    trace("PHASE_PRAMIN_PROBE: reading PRAMIN (no writes)")

    # First: read BAR0_WINDOW register (always safe)
    win = mmio_read(0x1700, "BAR0_WINDOW")
    trace(f"PRAMIN_PROBE: BAR0_WINDOW={win:#010x}")

    # Now try ember.pramin.read — 4 bytes from VRAM 0x10000
    trace("PRAMIN_PROBE: requesting ember.pramin.read(vram=0x10000, length=4)")
    try:
        rb = pramin_read(0x10000, 4)
        val = struct.unpack("<I", rb)[0] if len(rb) >= 4 else 0xDEAD
        trace(f"PRAMIN_PROBE: read returned {val:#010x}")
    except Exception as e:
        trace(f"PRAMIN_PROBE: read FAILED (graceful): {e}")
        return False

    if val == 0xFFFFFFFF:
        trace("PRAMIN_PROBE: VRAM unresponsive (0xFFFFFFFF)")
        return False
    if val & 0xFFF00000 == 0xBAD00000:
        trace(f"PRAMIN_PROBE: VRAM dead — PRI timeout pattern ({val:#010x})")
        return False

    trace("PRAMIN_PROBE: VRAM appears alive")
    return True

# ─── Phase B_SAFE: SEC2 reset (no PRAMIN writes) ────────────────────
def phase_b_safe():
    """SEC2 PMC reset + ENGCTL + scrub + halt — everything before PRAMIN writes."""
    trace("PHASE_B_SAFE: SEC2 reset sequence (no PRAMIN)")

    # PRI drain
    trace("B_SAFE: PRI drain")
    mmio_write(0x12004C, 0x2, "PRI_ACK")
    time.sleep(0.005)
    boot0 = mmio_read(0x0, "BOOT0_verify")
    if boot0 == 0xFFFFFFFF:
        trace("B_SAFE: PRI drain FAILED")
        return False

    # Read current PMC
    pmc = mmio_read(0x200, "PMC_ENABLE")
    sec2_mask = 1 << 5

    # MC disable SEC2
    trace("B_SAFE: MC disable SEC2")
    mmio_write(0x200, pmc & ~sec2_mask, "PMC_DISABLE_SEC2")
    mmio_read(0x200, "PMC_flush")
    time.sleep(0.001)
    mmio_write(0x12004C, 0x2, "PRI_ACK_post_disable")
    time.sleep(0.005)

    # ENGCTL blind toggle
    trace("B_SAFE: ENGCTL blind toggle")
    mmio_write(SEC2_BASE + 0x0A4, 0x01, "ENGCTL_set")  # 0x0A4 is ENGCTL relative
    time.sleep(0.0001)
    mmio_write(SEC2_BASE + 0x0A4, 0x00, "ENGCTL_clear")

    # MC enable SEC2
    trace("B_SAFE: MC enable SEC2")
    mmio_write(0x200, pmc | sec2_mask, "PMC_ENABLE_SEC2")
    mmio_read(0x200, "PMC_flush")
    time.sleep(0.001)
    mmio_write(0x12004C, 0x2, "PRI_ACK_post_enable")
    time.sleep(0.005)

    # Wait for scrub
    trace("B_SAFE: waiting for scrub")
    for _ in range(500):
        dmactl = mmio_read(SEC2_BASE + 0x10C, "DMACTL")
        if dmactl & 0x06 == 0:
            break
        time.sleep(0.0001)

    # Write BOOT0 to debug register
    boot0 = mmio_read(0x0, "BOOT0")
    mmio_write(SEC2_BASE + 0x408, boot0, "SEC2_DEBUG")

    # Wait for halt
    trace("B_SAFE: waiting for ROM halt")
    halted = False
    for _ in range(3000):
        cpuctl = mmio_read(SEC2_BASE + 0x100, "CPUCTL")
        if cpuctl & 0x10:
            halted = True
            break
        time.sleep(0.001)

    trace(f"B_SAFE: halted={halted}")

    # FBIF
    trace("B_SAFE: FBIF → PHYS_VID")
    fbif = mmio_read(SEC2_BASE + 0x624, "FBIF_TRANSCFG")
    mmio_write(SEC2_BASE + 0x624, (fbif & ~0x03) | 0x01, "FBIF_PHYS_VID")

    trace("PHASE_B_SAFE: complete — ready for PRAMIN")
    return halted

# ─── Phase B_PRAMIN: Write pages to VRAM ─────────────────────────────
def phase_b_pramin(num_pages=1):
    """Write zero pages to VRAM via ember.pramin.write — test write volume."""
    trace(f"PHASE_B_PRAMIN: writing {num_pages} zero page(s) to VRAM")
    page = b"\x00" * 4096
    page_addrs = [0x10000, 0x11000, 0x12000, 0x13000, 0x14000, 0x15000]

    for i in range(min(num_pages, len(page_addrs))):
        addr = page_addrs[i]
        trace(f"B_PRAMIN: writing page {i+1}/{num_pages} @ VRAM {addr:#x}")
        try:
            n = pramin_write(addr, page)
            trace(f"B_PRAMIN: page {i+1} written ({n} bytes)")
        except Exception as e:
            trace(f"B_PRAMIN: page {i+1} FAILED: {e}")
            return False

        # Verify: read first word back
        trace(f"B_PRAMIN: verifying page {i+1}")
        try:
            rb = pramin_read(addr, 4)
            val = struct.unpack("<I", rb)[0] if len(rb) >= 4 else 0xDEAD
            trace(f"B_PRAMIN: page {i+1} readback = {val:#010x}")
        except Exception as e:
            trace(f"B_PRAMIN: page {i+1} verify FAILED: {e}")
            return False

    trace(f"PHASE_B_PRAMIN: {num_pages} pages written OK")
    return True

# ─── Phase B_BIND: falcon v1 bind context ────────────────────────────
def phase_b_bind():
    """Full sec2.prepare_physical via ember RPC (includes PRAMIN + bind)."""
    trace("PHASE_B_BIND: calling ember.sec2.prepare_physical")
    try:
        r = ember_rpc("ember.sec2.prepare_physical", {"bdf": BDF}, timeout=30)
        ok = r.get("ok", False)
        notes = r.get("notes", [])
        for i, note in enumerate(notes):
            trace(f"B_BIND note[{i}]: {note}")
        trace(f"PHASE_B_BIND: ok={ok}")
        return ok
    except Exception as e:
        trace(f"PHASE_B_BIND: FAILED: {e}")
        return False

# ─── Main ────────────────────────────────────────────────────────────
def main():
    phase = sys.argv[1] if len(sys.argv) > 1 else "all"

    # Clear trace
    try:
        os.remove(TRACE)
    except FileNotFoundError:
        pass

    trace(f"exp145_decomposed START — phase={phase} bdf={BDF}")

    if phase in ("all", "a"):
        if not phase_a():
            trace("ABORT at phase A")
            return
        if phase == "a":
            trace("STOP after phase A (requested)")
            return

    if phase in ("all", "pramin_probe"):
        ok = phase_pramin_probe()
        if phase == "pramin_probe":
            trace(f"STOP after PRAMIN_PROBE (result={ok})")
            return
        if not ok:
            trace("WARNING: PRAMIN probe failed — GPU VRAM is cold/dead")
            if phase == "all":
                trace("ABORT — refusing to continue with dead VRAM")
                return

    if phase in ("all", "b_safe"):
        if not phase_b_safe():
            trace("ABORT at phase B_SAFE")
            return
        if phase == "b_safe":
            trace("STOP after phase B_SAFE (requested)")
            return

    if phase in ("all", "b_pramin_1page"):
        if not phase_b_pramin(1):
            trace("ABORT at B_PRAMIN (1 page)")
            return
        if phase == "b_pramin_1page":
            trace("STOP after B_PRAMIN 1 page (requested)")
            return

    if phase in ("all", "b_pramin_all"):
        if not phase_b_pramin(6):
            trace("ABORT at B_PRAMIN (6 pages)")
            return
        if phase == "b_pramin_all":
            trace("STOP after B_PRAMIN 6 pages (requested)")
            return

    if phase in ("all", "b_bind"):
        if not phase_b_bind():
            trace("ABORT at B_BIND")
            return
        if phase == "b_bind":
            trace("STOP after B_BIND (requested)")
            return

    trace("exp145_decomposed COMPLETE")
    print("\n  All phases completed successfully!")

if __name__ == "__main__":
    main()
