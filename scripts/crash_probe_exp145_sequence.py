#!/usr/bin/env python3
"""Replicate exp145's EXACT pre-crash sequence to find the crash vector.

exp145 does:
  1. experiment_start (glowplug)
  2. Read BOOT0, PMC, SEC2_CPUCTL
  3. Read FECS falcon state (returns 0xbadf1201 PRI faults)
  4. Read GPCCS falcon state (returns 0xbadf1200 PRI faults)
  5. Read PMC_ENABLE again
  6. Call sec2.prepare_physical  <-- CRASH HERE

This script does the same sequence, with fsync logging at each step.
"""

import socket, json, sys, os, time

LOG_PATH = "/var/lib/coralreef/traces/crash_probe_seq.log"
EMBER = "/run/coralreef/ember.sock"
GLOWPLUG = "/run/coralreef/glowplug.sock"

def log(msg):
    ts = int(time.time() * 1000)
    line = f"[{ts}] {msg}\n"
    sys.stderr.write(f"  {msg}\n")
    with open(LOG_PATH, "a") as f:
        f.write(line)
        f.flush()
        os.fsync(f.fileno())

def rpc(sock_path, method, params=None, timeout=10):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.settimeout(timeout)
    s.connect(sock_path)
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

BDF = "0000:03:00.0"

def r32(offset, label):
    result = rpc(EMBER, "ember.mmio.read", {"bdf": BDF, "offset": offset})
    val = result.get("value", 0xDEADDEAD)
    log(f"r32 {label} @ {offset:#x} = {val:#010x}")
    return val

def batch_read(offsets_labels):
    ops = [{"op": "r", "offset": off, "value": 0} for off, _ in offsets_labels]
    result = rpc(EMBER, "ember.mmio.batch", {"bdf": BDF, "ops": ops})
    values = result if isinstance(result, list) else result.get("results", [])
    for i, (off, label) in enumerate(offsets_labels):
        val = values[i] if i < len(values) else "?"
        log(f"batch {label} @ {off:#x} = {val}")
    return values

def main():
    with open(LOG_PATH, "w") as f:
        f.write("")

    log("╔══════════════════════════════════════════════════════════╗")
    log("║  Exp 150b: Replicate exp145 pre-crash sequence          ║")
    log("╚══════════════════════════════════════════════════════════╝")

    # Step 1: experiment_start via glowplug
    log("STEP 1: experiment_start via glowplug")
    try:
        result = rpc(GLOWPLUG, "device.experiment_start",
                     {"bdf": BDF, "name": "crash_probe_seq", "watchdog_secs": 120})
        log(f"  experiment_start OK: {result}")
    except Exception as e:
        log(f"  experiment_start FAILED: {e} (continuing anyway)")

    # Step 2: Read identity registers (like exp145 Phase A)
    log("STEP 2: Identity reads")
    boot0 = r32(0x0, "BOOT0")
    pmc = r32(0x200, "PMC_ENABLE")
    sec2_cpuctl = r32(0x87100, "SEC2_CPUCTL")
    log(f"  BOOT0={boot0:#010x} PMC={pmc:#010x} SEC2={sec2_cpuctl:#010x}")

    # Step 3: Read FECS falcon state (PRI faults expected)
    log("STEP 3: FECS falcon state (expect PRI faults)")
    fecs_ops = [
        (0x409100, "FECS_CPUCTL"),
        (0x409240, "FECS_SCTL"),
        (0x409030, "FECS_PC"),
        (0x409148, "FECS_EXCI"),
    ]
    batch_read(fecs_ops)

    # Step 4: Read GPCCS falcon state (PRI faults expected)
    log("STEP 4: GPCCS falcon state (expect PRI faults)")
    gpccs_ops = [
        (0x41A100, "GPCCS_CPUCTL"),
        (0x41A240, "GPCCS_SCTL"),
        (0x41A030, "GPCCS_PC"),
        (0x41A148, "GPCCS_EXCI"),
    ]
    batch_read(gpccs_ops)

    # Step 5: Read PMC again (like exp145 does before Phase B)
    log("STEP 5: PMC_ENABLE pre-Phase-B")
    pmc2 = r32(0x200, "PMC_PRE_PHASE_B")

    # Step 6: THE CRITICAL CALL
    log("STEP 6: CALLING ember.sec2.prepare_physical — THIS IS WHERE EXP145 CRASHES")
    t0 = time.time()
    try:
        result = rpc(EMBER, "ember.sec2.prepare_physical", {"bdf": BDF}, timeout=15)
        dt = (time.time() - t0) * 1000
        ok = result.get("ok", False)
        notes = result.get("notes", [])
        log(f"STEP 6 RESULT: ok={ok} ({dt:.1f}ms)")
        for i, n in enumerate(notes):
            log(f"  NOTE[{i}]: {n}")
    except Exception as e:
        dt = (time.time() - t0) * 1000
        log(f"STEP 6 EXCEPTION: {e} ({dt:.1f}ms)")

    log("STEP 6 SURVIVED")

    # Step 7: experiment_end
    log("STEP 7: experiment_end")
    try:
        rpc(GLOWPLUG, "device.experiment_end", {"bdf": BDF})
        log("  experiment_end OK")
    except Exception as e:
        log(f"  experiment_end FAILED: {e}")

    log("╔══════════════════════════════════════════════════════════╗")
    log("║  SEQUENCE COMPLETE — SYSTEM SURVIVED                    ║")
    log("╚══════════════════════════════════════════════════════════╝")

if __name__ == "__main__":
    main()
