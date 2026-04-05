#!/usr/bin/env python3
"""Experiment 150: Systematic crash vector hunt.

Probes GPU registers one-by-one via ember RPC, with fsync logging before
each operation. If the system freezes, the last "ATTEMPT" line in the log
identifies the crash vector.

Usage:
    python3 scripts/crash_probe.py [--phase N] [--bdf 0000:03:00.0]
    
    --phase N   Start from phase N (default: 1). Use after a crash to
                resume from the phase that killed us.
    --skip N    Skip specific phase N (can repeat: --skip 13 --skip 14)
"""

import socket, json, sys, os, time, argparse

LOG_PATH = "/var/lib/coralreef/traces/crash_probe.log"
SOCK = "/run/coralreef/ember.sock"

def log(msg, flush=True):
    ts = int(time.time() * 1000)
    line = f"[{ts}] {msg}\n"
    sys.stderr.write(f"  {msg}\n")
    with open(LOG_PATH, "a") as f:
        f.write(line)
        f.flush()
        os.fsync(f.fileno())

def rpc(method, params=None, timeout=5):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    s.connect(SOCK)
    req = json.dumps({"jsonrpc": "2.0", "method": method,
                       "params": params or {}, "id": 1})
    s.sendall(req.encode() + b"\n")
    buf = b""
    while b"\n" not in buf:
        chunk = s.recv(4096)
        if not chunk:
            break
        buf += chunk
    s.close()
    resp = json.loads(buf)
    if "error" in resp:
        raise RuntimeError(resp["error"].get("message", str(resp["error"])))
    return resp.get("result")

def r32(bdf, offset, label):
    log(f"ATTEMPT r32 {label} @ {offset:#x}")
    t0 = time.time()
    result = rpc("ember.mmio.read", {"bdf": bdf, "offset": offset})
    dt = (time.time() - t0) * 1000
    val = result.get("value", 0xDEADDEAD)
    dead = val == 0xFFFFFFFF or (val >> 16) in (0xBAD0, 0xBADF, 0xBAD1, 0xDEAD)
    tag = "DEAD" if dead else "OK"
    log(f"RESULT r32 {label} @ {offset:#x} = {val:#010x} ({dt:.1f}ms) [{tag}]")
    return val

def w32(bdf, offset, value, label):
    log(f"ATTEMPT w32 {label} @ {offset:#x} = {value:#010x}")
    t0 = time.time()
    rpc("ember.mmio.write", {"bdf": bdf, "offset": offset, "value": value})
    dt = (time.time() - t0) * 1000
    log(f"RESULT w32 {label} @ {offset:#x} ({dt:.1f}ms)")

def batch(bdf, ops, label):
    desc = ", ".join(f"{'r' if o[0]=='r' else 'w'}@{o[1]:#x}" for o in ops[:4])
    if len(ops) > 4:
        desc += f" +{len(ops)-4} more"
    log(f"ATTEMPT batch [{label}]: {desc}")
    t0 = time.time()
    params = {"bdf": bdf, "ops": [{"op": o[0], "offset": o[1], "value": o[2]} for o in ops]}
    result = rpc("ember.mmio.batch", params)
    dt = (time.time() - t0) * 1000
    log(f"RESULT batch [{label}] ({dt:.1f}ms): {len(result)} results")
    return result

# ══════════════════════════════════════════════════════════════
# Probe phases
# ══════════════════════════════════════════════════════════════

def phase_1(bdf):
    """P1: Identity — BOOT0 only"""
    log("═══ PHASE 1: BOOT0 (safest possible read) ═══")
    v = r32(bdf, 0x0, "BOOT0")
    assert v != 0xFFFFFFFF, "BOOT0 returned all-ones — GPU not responding"
    log(f"P1 PASS: BOOT0={v:#010x}")

def phase_2(bdf):
    """P2: PMC registers"""
    log("═══ PHASE 2: PMC registers ═══")
    r32(bdf, 0x200, "PMC_ENABLE")
    r32(bdf, 0x20C, "PMC_DEV_ENABLE")
    r32(bdf, 0x204, "PMC_DEV_ENABLE_STATUS")
    log("P2 PASS")

def phase_3(bdf):
    """P3: PTIMER"""
    log("═══ PHASE 3: PTIMER ═══")
    r32(bdf, 0x9400, "PTIMER_TIME_0")
    r32(bdf, 0x9410, "PTIMER_TIME_1")
    log("P3 PASS")

def phase_4(bdf):
    """P4: PFIFO"""
    log("═══ PHASE 4: PFIFO ═══")
    r32(bdf, 0x2004, "PFIFO_CTRL")
    r32(bdf, 0x2100, "PFIFO_INTR_0")
    r32(bdf, 0x2504, "PFIFO_SCHED")
    log("P4 PASS")

def phase_5(bdf):
    """P5: PBDMA"""
    log("═══ PHASE 5: PBDMA ═══")
    r32(bdf, 0x40108, "PBDMA0_STATUS")
    r32(bdf, 0x40148, "PBDMA0_INTR")
    log("P5 PASS")

def phase_6(bdf):
    """P6: PFB / FBHUB"""
    log("═══ PHASE 6: PFB + FBHUB ═══")
    r32(bdf, 0x100000, "PFB_CFG0")
    r32(bdf, 0x100800, "FBHUB_CFG")
    r32(bdf, 0x100C80, "PFB_NISO")
    log("P6 PASS")

def phase_7(bdf):
    """P7: SEC2 falcon"""
    log("═══ PHASE 7: SEC2 falcon registers ═══")
    base = 0x87000
    r32(bdf, base + 0x100, "SEC2_CPUCTL")
    r32(bdf, base + 0x240, "SEC2_SCTL")
    r32(bdf, base + 0x030, "SEC2_PC")
    r32(bdf, base + 0x148, "SEC2_EXCI")
    r32(bdf, base + 0x108, "SEC2_HWCFG")
    r32(bdf, base + 0x040, "SEC2_MAILBOX0")
    r32(bdf, base + 0x044, "SEC2_MAILBOX1")
    log("P7 PASS")

def phase_8(bdf):
    """P8: FECS falcon"""
    log("═══ PHASE 8: FECS falcon registers ═══")
    base = 0x409000
    r32(bdf, base + 0x100, "FECS_CPUCTL")
    r32(bdf, base + 0x240, "FECS_SCTL")
    r32(bdf, base + 0x030, "FECS_PC")
    r32(bdf, base + 0x148, "FECS_EXCI")
    log("P8 PASS")

def phase_9(bdf):
    """P9: GPCCS falcon"""
    log("═══ PHASE 9: GPCCS falcon registers ═══")
    base = 0x41A000
    r32(bdf, base + 0x100, "GPCCS_CPUCTL")
    r32(bdf, base + 0x240, "GPCCS_SCTL")
    r32(bdf, base + 0x030, "GPCCS_PC")
    r32(bdf, base + 0x148, "GPCCS_EXCI")
    log("P9 PASS")

def phase_10(bdf):
    """P10: LTC + FBPA"""
    log("═══ PHASE 10: LTC + FBPA ═══")
    r32(bdf, 0x17E200, "LTC0_CFG")
    r32(bdf, 0x9A0000, "FBPA0_CFG")
    r32(bdf, 0x17E264, "LTC0_TSTG_CFG1")
    r32(bdf, 0x9A0004, "FBPA0_STATUS")
    log("P10 PASS")

def phase_11(bdf):
    """P11: PRAMIN read (set window + read VRAM through BAR0)"""
    log("═══ PHASE 11: PRAMIN read (window + VRAM) ═══")
    saved = r32(bdf, 0x1700, "BAR0_WINDOW_save")
    w32(bdf, 0x1700, 0, "BAR0_WINDOW=0")
    time.sleep(0.005)
    for i in range(4):
        r32(bdf, 0x700000 + i*4, f"PRAMIN[{i}]")
    w32(bdf, 0x1700, saved, "BAR0_WINDOW_restore")
    log("P11 PASS")

def phase_12(bdf):
    """P12: PRAMIN write + readback"""
    log("═══ PHASE 12: PRAMIN write + readback ═══")
    w32(bdf, 0x1700, 0, "BAR0_WINDOW=0")
    time.sleep(0.005)

    test_vals = [0xDEADBEEF, 0xCAFEBABE, 0x12345678, 0xA5A5A5A5]
    for i, val in enumerate(test_vals):
        w32(bdf, 0x700000 + i*4, val, f"PRAMIN_WRITE[{i}]")

    time.sleep(0.005)
    for i, expected in enumerate(test_vals):
        got = r32(bdf, 0x700000 + i*4, f"PRAMIN_READBACK[{i}]")
        if got != expected:
            log(f"P12 MISMATCH: [{i}] wrote {expected:#010x} got {got:#010x}")

    # Restore zeros
    for i in range(4):
        w32(bdf, 0x700000 + i*4, 0, f"PRAMIN_ZERO[{i}]")
    log("P12 PASS")

def phase_13(bdf):
    """P13: PMC SEC2 reset (toggle bit 5) — DANGER"""
    log("═══ PHASE 13: PMC SEC2 reset (bit 5 toggle) — DANGER ═══")
    pmc = r32(bdf, 0x200, "PMC_ENABLE_before")
    sec2_bit = 1 << 5

    log(f"ATTEMPT: clear SEC2 bit (PMC &= ~bit5)")
    w32(bdf, 0x200, pmc & ~sec2_bit, "PMC_ENABLE_clear_bit5")
    time.sleep(0.01)

    log(f"ATTEMPT: set SEC2 bit (PMC |= bit5)")
    w32(bdf, 0x200, pmc | sec2_bit, "PMC_ENABLE_set_bit5")
    time.sleep(0.01)

    pmc_after = r32(bdf, 0x200, "PMC_ENABLE_after")
    log(f"P13 PASS: PMC {pmc:#010x} → {pmc_after:#010x}")

def phase_14(bdf):
    """P14: PRI ring status — KNOWN LETHAL after nouveau"""
    log("═══ PHASE 14: PRI_RING_INTR_STATUS (0x120058) — KNOWN LETHAL ═══")
    log("WARNING: This register is known to cause system lockups after nouveau teardown")
    log("ATTEMPT r32 PRI_RING_INTR_STATUS @ 0x120058")
    r32(bdf, 0x120058, "PRI_RING_INTR_STATUS")
    log("P14 PASS (surprisingly)")

def phase_15(bdf):
    """P15: SEC2 prepare_physical via ember (the full reset sequence)"""
    log("═══ PHASE 15: ember.sec2.prepare_physical (full reset sequence) ═══")
    log("ATTEMPT: calling ember.sec2.prepare_physical")
    t0 = time.time()
    result = rpc("ember.sec2.prepare_physical", {"bdf": bdf}, timeout=15)
    dt = (time.time() - t0) * 1000
    ok = result.get("ok", False)
    notes = result.get("notes", [])
    log(f"RESULT: ok={ok} ({dt:.1f}ms) notes={len(notes)}")
    for i, note in enumerate(notes):
        log(f"  NOTE[{i}]: {note}")
    log(f"P15 {'PASS' if ok else 'FAIL'}")

def phase_16(bdf):
    """P16: Bulk PRAMIN write via ember (like exp145 phase C1)"""
    log("═══ PHASE 16: Bulk PRAMIN write (WPR-sized, via ember.pramin.write) ═══")
    import base64
    # Write 4KB of test pattern to VRAM 0x80000 (WPR location)
    pattern = bytes(range(256)) * 16  # 4KB
    encoded = base64.b64encode(pattern).decode()
    log(f"ATTEMPT: ember.pramin.write 4KB to VRAM 0x80000")
    t0 = time.time()
    result = rpc("ember.pramin.write",
                  {"bdf": bdf, "vram_addr": 0x80000, "data_b64": encoded},
                  timeout=15)
    dt = (time.time() - t0) * 1000
    written = result.get("bytes_written", 0)
    log(f"RESULT: {written} bytes written ({dt:.1f}ms)")
    log(f"P16 {'PASS' if written == 4096 else 'FAIL'}")

def phase_17(bdf):
    """P17: Falcon IMEM/DMEM upload via ember"""
    log("═══ PHASE 17: Falcon IMEM+DMEM upload via ember ═══")
    import base64
    # Upload 256 bytes of test data to SEC2 IMEM
    data = bytes([0] * 256)
    encoded = base64.b64encode(data).decode()

    log("ATTEMPT: ember.falcon.upload_imem (256B to SEC2)")
    t0 = time.time()
    try:
        rpc("ember.falcon.upload_imem",
            {"bdf": bdf, "base": 0x87000, "offset": 0, "data_b64": encoded, "start_tag": 0},
            timeout=10)
        dt = (time.time() - t0) * 1000
        log(f"RESULT: IMEM upload OK ({dt:.1f}ms)")
    except Exception as e:
        dt = (time.time() - t0) * 1000
        log(f"RESULT: IMEM upload FAILED: {e} ({dt:.1f}ms)")

    log("ATTEMPT: ember.falcon.upload_dmem (256B to SEC2)")
    t0 = time.time()
    try:
        rpc("ember.falcon.upload_dmem",
            {"bdf": bdf, "base": 0x87000, "offset": 0, "data_b64": encoded},
            timeout=10)
        dt = (time.time() - t0) * 1000
        log(f"RESULT: DMEM upload OK ({dt:.1f}ms)")
    except Exception as e:
        dt = (time.time() - t0) * 1000
        log(f"RESULT: DMEM upload FAILED: {e} ({dt:.1f}ms)")

    log("P17 PASS")

def phase_18(bdf):
    """P18: Falcon CPU start via ember — the actual boot"""
    log("═══ PHASE 18: falcon_start_cpu via ember (SEC2 boot) ═══")
    # First set bootvec to 0 (safe: falcon will re-halt immediately)
    w32(bdf, 0x87000 + 0x104, 0, "SEC2_BOOTVEC=0")
    time.sleep(0.005)

    log("ATTEMPT: ember.falcon.start_cpu")
    t0 = time.time()
    try:
        result = rpc("ember.falcon.start_cpu", {"bdf": bdf, "base": 0x87000}, timeout=10)
        dt = (time.time() - t0) * 1000
        pc = result.get("pc", "?")
        exci = result.get("exci", "?")
        log(f"RESULT: start_cpu OK pc={pc} exci={exci} ({dt:.1f}ms)")
    except Exception as e:
        dt = (time.time() - t0) * 1000
        log(f"RESULT: start_cpu FAILED: {e} ({dt:.1f}ms)")

    log("P18 PASS")

def phase_19(bdf):
    """P19: prepare_dma via ember (enables bus_master)"""
    log("═══ PHASE 19: ember.prepare_dma (bus_master enable) ═══")
    log("ATTEMPT: ember.prepare_dma")
    t0 = time.time()
    try:
        result = rpc("ember.prepare_dma", {"bdf": bdf}, timeout=10)
        dt = (time.time() - t0) * 1000
        log(f"RESULT: prepare_dma OK ({dt:.1f}ms): {result}")
    except Exception as e:
        dt = (time.time() - t0) * 1000
        log(f"RESULT: prepare_dma FAILED: {e} ({dt:.1f}ms)")

    log("P19 PASS")

    # Cleanup
    log("ATTEMPT: ember.cleanup_dma")
    try:
        rpc("ember.cleanup_dma", {"bdf": bdf}, timeout=10)
        log("RESULT: cleanup_dma OK")
    except Exception as e:
        log(f"RESULT: cleanup_dma FAILED: {e}")


PHASES = {
    1: phase_1, 2: phase_2, 3: phase_3, 4: phase_4, 5: phase_5,
    6: phase_6, 7: phase_7, 8: phase_8, 9: phase_9, 10: phase_10,
    11: phase_11, 12: phase_12, 13: phase_13, 14: phase_14,
    15: phase_15, 16: phase_16, 17: phase_17, 18: phase_18, 19: phase_19,
}

def main():
    parser = argparse.ArgumentParser(description="Exp 150: Crash vector hunt")
    parser.add_argument("--phase", type=int, default=1, help="Start phase")
    parser.add_argument("--end", type=int, default=19, help="End phase")
    parser.add_argument("--skip", type=int, action="append", default=[], help="Skip phase(s)")
    parser.add_argument("--bdf", default="0000:03:00.0")
    parser.add_argument("--only", type=int, help="Run only this phase")
    args = parser.parse_args()

    os.makedirs(os.path.dirname(LOG_PATH), exist_ok=True)
    # Clear previous log
    with open(LOG_PATH, "w") as f:
        f.write("")

    log(f"╔══════════════════════════════════════════════════════════╗")
    log(f"║  Experiment 150: Crash Vector Hunt                      ║")
    log(f"║  BDF: {args.bdf}                                  ║")
    log(f"║  Phases: {args.phase}-{args.end} (skip: {args.skip})                      ║")
    log(f"╚══════════════════════════════════════════════════════════╝")

    if args.only:
        phases_to_run = [args.only]
    else:
        phases_to_run = range(args.phase, args.end + 1)

    for p in phases_to_run:
        if p in args.skip:
            log(f"SKIP phase {p}")
            continue
        if p not in PHASES:
            log(f"SKIP phase {p} (not defined)")
            continue
        try:
            PHASES[p](args.bdf)
        except Exception as e:
            log(f"PHASE {p} EXCEPTION: {e}")
            # Continue to next phase
        log(f"──── survived phase {p} ────")
        time.sleep(0.5)

    log("╔══════════════════════════════════════════════════════════╗")
    log("║  ALL PHASES COMPLETE — SYSTEM SURVIVED                  ║")
    log("╚══════════════════════════════════════════════════════════╝")

if __name__ == "__main__":
    main()
