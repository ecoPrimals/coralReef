#!/usr/bin/env python3
"""Ember Containment Stress Test — deliberately trigger every known crash vector.

For each vector:
  1. Check ember is alive
  2. Trigger the dangerous operation
  3. Verify system survived (we're still running)
  4. Check if ember survived or sacrificed
  5. If ember died, wait for glowplug resurrection
  6. Log result

Crash vectors organized by severity tier:

  TIER 1 — SAFE: reads to various register domains, PRI faults expected
  TIER 2 — DANGEROUS: PMC toggles, PRAMIN ops, engine resets
  TIER 3 — LETHAL: bad VRAM addresses, power-gated writes, rapid-fire storms

Usage:
  sudo python3 scripts/stress_containment.py [tier]
  tier = all | 1 | 2 | 3 | vector_name
"""

import socket, json, sys, os, time, struct, base64, traceback

EMBER = "/run/coralreef/ember.sock"
GLOWPLUG = "/run/coralreef/glowplug.sock"
TRACE_DIR = "/var/lib/coralreef/traces"

TITAN_V = "0000:03:00.0"
K80_DIE1 = "0000:4c:00.0"
K80_DIE2 = "0000:4d:00.0"

results = []

def log(msg):
    ts = time.strftime("%H:%M:%S")
    line = f"[{ts}] {msg}"
    print(line, flush=True)

def rpc(sock_path, method, params=None, timeout=30):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(sock_path)
    s.settimeout(timeout)
    req = json.dumps({"jsonrpc": "2.0", "method": method, "params": params or {}, "id": 1})
    s.sendall((req + "\n").encode())
    buf = b""
    while b"\n" not in buf and len(buf) < 1048576:
        chunk = s.recv(16384)
        if not chunk:
            break
        buf += chunk
    s.close()
    if not buf:
        return {"error": {"message": "empty response — ember likely died"}}
    return json.loads(buf)

def ember_alive():
    """Quick heartbeat check."""
    try:
        r = rpc(EMBER, "ember.status", timeout=5)
        return "error" not in r
    except Exception:
        return False

def wait_for_resurrection(max_wait=60):
    """Wait for glowplug to resurrect ember after a sacrificial death."""
    log("  EMBER DIED — waiting for glowplug resurrection...")
    for i in range(max_wait):
        time.sleep(1)
        if ember_alive():
            log(f"  RESURRECTED after {i+1}s")
            return True
    log(f"  RESURRECTION FAILED after {max_wait}s")
    return False

def run_vector(name, tier, bdf, fn):
    """Execute a single crash vector with full containment tracking."""
    log(f"\n{'='*60}")
    log(f"VECTOR: {name} (Tier {tier}) on {bdf}")
    log(f"{'='*60}")

    # Pre-check
    alive_before = ember_alive()
    if not alive_before:
        log("  PRE-CHECK: ember not alive, waiting for resurrection first")
        if not wait_for_resurrection():
            results.append((name, tier, bdf, "SKIP", "ember not alive"))
            return
        alive_before = True

    # Execute
    t0 = time.time()
    try:
        outcome = fn(bdf)
        dt = time.time() - t0
        log(f"  RETURNED in {dt:.1f}s: {outcome}")
    except Exception as e:
        dt = time.time() - t0
        outcome = f"EXCEPTION: {e}"
        log(f"  EXCEPTION in {dt:.1f}s: {e}")

    # Post-check
    time.sleep(0.5)
    alive_after = ember_alive()

    if alive_after:
        log(f"  RESULT: SYSTEM ALIVE, EMBER ALIVE")
        results.append((name, tier, bdf, "CONTAINED", str(outcome)[:80]))
    else:
        log(f"  RESULT: SYSTEM ALIVE, EMBER SACRIFICED")
        resurrected = wait_for_resurrection()
        status = "SACRIFICED+RESURRECTED" if resurrected else "SACRIFICED+DEAD"
        results.append((name, tier, bdf, status, str(outcome)[:80]))


# ═══════════════════════════════════════════════════════════════
# TIER 1 — SAFE: reads that should never crash
# ═══════════════════════════════════════════════════════════════

def t1_boot0_read(bdf):
    """Read BOOT0 identity register — safest possible BAR0 op."""
    r = rpc(EMBER, "ember.mmio.read", {"bdf": bdf, "offset": 0x0})
    return r.get("result", r.get("error", {}))

def t1_pmc_read(bdf):
    """Read PMC_ENABLE — always accessible."""
    r = rpc(EMBER, "ember.mmio.read", {"bdf": bdf, "offset": 0x200})
    return r.get("result", r.get("error", {}))

def t1_pri_fault_read(bdf):
    """Read PRI-faulted FECS register — returns 0xbadfXXXX, should not crash."""
    r = rpc(EMBER, "ember.mmio.read", {"bdf": bdf, "offset": 0x409100})
    return r.get("result", r.get("error", {}))

def t1_pgraph_read(bdf):
    """Read power-gated PGRAPH register — PRI fault expected."""
    r = rpc(EMBER, "ember.mmio.read", {"bdf": bdf, "offset": 0x400100})
    return r.get("result", r.get("error", {}))

def t1_batch_safe_reads(bdf):
    """Batch of 20 safe register reads."""
    ops = [{"type": "r", "offset": off, "value": 0} for off in [
        0x0, 0x4, 0x8, 0x200, 0x204,
        0x8800, 0x8804, 0x022430, 0x022434, 0x022438,
        0x100, 0x104, 0x108, 0x10C, 0x110,
        0x88000, 0x88004, 0x88008, 0x8800C, 0x88010,
    ]]
    r = rpc(EMBER, "ember.mmio.batch", {"bdf": bdf, "ops": ops})
    return f"batch_ok, {len(ops)} ops"

def t1_rapid_fire_reads(bdf):
    """100 sequential BOOT0 reads — stress fork isolation throughput."""
    for i in range(100):
        r = rpc(EMBER, "ember.mmio.read", {"bdf": bdf, "offset": 0x0})
        if "error" in r:
            return f"failed at iteration {i}: {r['error']}"
    return "100 reads OK"

def t1_batch_pri_faults(bdf):
    """Batch read of 10 registers in power-gated domains."""
    ops = [{"type": "r", "offset": off, "value": 0} for off in [
        0x409100, 0x409240, 0x41A100, 0x41A240,
        0x400100, 0x400200, 0x400300,
        0x500000, 0x500004, 0x500008,
    ]]
    r = rpc(EMBER, "ember.mmio.batch", {"bdf": bdf, "ops": ops})
    return f"batch_pri_faults done"


# ═══════════════════════════════════════════════════════════════
# TIER 2 — DANGEROUS: state-modifying, may fault device
# ═══════════════════════════════════════════════════════════════

def t2_pmc_toggle(bdf):
    """PMC_ENABLE toggle — disable SEC2 then re-enable. Known to dirty state."""
    r = rpc(EMBER, "ember.mmio.read", {"bdf": bdf, "offset": 0x200})
    val = r.get("result", {}).get("value", 0)
    # Disable SEC2 bit (bit 5)
    disabled = val & ~(1 << 5)
    rpc(EMBER, "ember.mmio.write", {"bdf": bdf, "offset": 0x200, "value": disabled})
    time.sleep(0.1)
    # Re-enable
    rpc(EMBER, "ember.mmio.write", {"bdf": bdf, "offset": 0x200, "value": val})
    return f"PMC toggled: {hex(val)} → {hex(disabled)} → {hex(val)}"

def t2_sec2_prepare(bdf):
    """Full sec2.prepare_physical — the exp145 core op."""
    r = rpc(EMBER, "ember.sec2.prepare_physical", {"bdf": bdf}, timeout=30)
    result = r.get("result", r.get("error", {}))
    ok = result.get("ok", False) if isinstance(result, dict) else False
    return f"sec2_prepare ok={ok}"

def t2_pramin_read(bdf):
    """Single PRAMIN read at safe VRAM offset."""
    r = rpc(EMBER, "ember.pramin.read", {"bdf": bdf, "vram_addr": 0x10000, "length": 4})
    result = r.get("result", r.get("error", {}))
    if isinstance(result, dict) and "data_b64" in result:
        raw = base64.b64decode(result["data_b64"])
        val = struct.unpack_from("<I", raw, 0)[0]
        return f"pramin_read={hex(val)}"
    return f"pramin_read result={result}"

def t2_pramin_read_4k(bdf):
    """PRAMIN read of 4KB — bulk VRAM access."""
    r = rpc(EMBER, "ember.pramin.read", {"bdf": bdf, "vram_addr": 0x10000, "length": 4096}, timeout=30)
    result = r.get("result", r.get("error", {}))
    if isinstance(result, dict) and "data_b64" in result:
        raw = base64.b64decode(result["data_b64"])
        return f"pramin_read_4k: {len(raw)} bytes"
    return f"pramin_read_4k: {result}"

def t2_pramin_write_safe(bdf):
    """PRAMIN write 4 bytes to known-safe VRAM scratch area. Reads back to verify."""
    payload = base64.b64encode(struct.pack("<I", 0xCAFEBABE)).decode()
    r = rpc(EMBER, "ember.pramin.write", {"bdf": bdf, "vram_addr": 0x10000, "data_b64": payload}, timeout=30)
    result = r.get("result", r.get("error", {}))
    return f"pramin_write result={result}"

def t2_falcon_start_no_fw(bdf):
    """falcon.start_cpu with no firmware loaded — SEC2 should be halted/stuck."""
    r = rpc(EMBER, "ember.falcon.start_cpu", {"bdf": bdf, "base": 0x87000}, timeout=15)
    result = r.get("result", r.get("error", {}))
    return f"falcon_start_no_fw: {result}"

def t2_falcon_poll_stuck(bdf):
    """falcon.poll on a halted falcon — bounded poll, should timeout cleanly."""
    r = rpc(EMBER, "ember.falcon.poll", {
        "bdf": bdf, "base": 0x87000,
        "timeout_ms": 2000, "mailbox_sentinel": 0xDEADA5A5
    }, timeout=15)
    result = r.get("result", r.get("error", {}))
    if isinstance(result, dict):
        final = result.get("final", {})
        return f"poll done: cpuctl={hex(final.get('cpuctl', 0))} pc={hex(final.get('pc', 0))}"
    return f"poll: {result}"


# ═══════════════════════════════════════════════════════════════
# TIER 3 — LETHAL: designed to trigger containment boundaries
# ═══════════════════════════════════════════════════════════════

def t3_pramin_bad_addr(bdf):
    """PRAMIN read at unmapped VRAM address — may stall PCIe."""
    r = rpc(EMBER, "ember.pramin.read", {"bdf": bdf, "vram_addr": 0xFFFF0000, "length": 4}, timeout=30)
    return r.get("result", r.get("error", {}))

def t3_pramin_write_bad_addr(bdf):
    """PRAMIN write to unmapped VRAM — most dangerous single operation."""
    payload = base64.b64encode(struct.pack("<I", 0xDEADBEEF)).decode()
    r = rpc(EMBER, "ember.pramin.write", {"bdf": bdf, "vram_addr": 0xFFFF0000, "data_b64": payload}, timeout=30)
    return r.get("result", r.get("error", {}))

def t3_write_power_gated(bdf):
    """Write to power-gated PGRAPH register — PRI fault, may degrade bus."""
    r = rpc(EMBER, "ember.mmio.write", {"bdf": bdf, "offset": 0x400100, "value": 0xDEADBEEF})
    return r.get("result", r.get("error", {}))

def t3_write_fb_pfb(bdf):
    """Write to PFB (frame buffer) controller — power-gated, known to degrade MC."""
    r = rpc(EMBER, "ember.mmio.write", {"bdf": bdf, "offset": 0x100C00, "value": 0x1})
    return r.get("result", r.get("error", {}))

def t3_rapid_fire_writes(bdf):
    """50 rapid writes to PRI_ACK — stress fork throughput on writes."""
    for i in range(50):
        r = rpc(EMBER, "ember.mmio.write", {"bdf": bdf, "offset": 0x12004C, "value": 0x2})
        if "error" in r:
            return f"failed at {i}: {r['error']}"
    return "50 PRI_ACK writes OK"

def t3_batch_dangerous_writes(bdf):
    """Batch write to mix of safe and power-gated registers."""
    ops = [
        {"type": "w", "offset": 0x12004C, "value": 0x2},     # PRI ACK (safe)
        {"type": "r", "offset": 0x0, "value": 0},             # BOOT0 (safe)
        {"type": "w", "offset": 0x400100, "value": 0x0},      # PGRAPH (power-gated)
        {"type": "r", "offset": 0x400100, "value": 0},        # read back
        {"type": "w", "offset": 0x12004C, "value": 0x2},      # PRI ACK again
    ]
    r = rpc(EMBER, "ember.mmio.batch", {"bdf": bdf, "ops": ops})
    return r.get("result", r.get("error", {}))

def t3_double_sec2_no_warm(bdf):
    """Two sec2_prepare calls without warm cycle — SEC2 PRI faulted on second."""
    r1 = rpc(EMBER, "ember.sec2.prepare_physical", {"bdf": bdf}, timeout=30)
    ok1 = r1.get("result", {}).get("ok", False)
    r2 = rpc(EMBER, "ember.sec2.prepare_physical", {"bdf": bdf}, timeout=30)
    ok2 = r2.get("result", {}).get("ok", False)
    return f"run1={ok1} run2={ok2}"

def t3_k80_engine_enable_pgraph(bdf):
    """K80: enable PGRAPH in PMC_ENABLE — power-gated, may PRI-hang."""
    r = rpc(EMBER, "ember.mmio.read", {"bdf": bdf, "offset": 0x200})
    pmc = r.get("result", {}).get("value", 0)
    new_pmc = pmc | (1 << 12)  # PGRAPH bit
    r = rpc(EMBER, "ember.mmio.write", {"bdf": bdf, "offset": 0x200, "value": new_pmc})
    time.sleep(0.2)
    # Read back PGRAPH status
    r2 = rpc(EMBER, "ember.mmio.read", {"bdf": bdf, "offset": 0x400100})
    pgraph = r2.get("result", {}).get("value", 0xDEAD)
    return f"PMC={hex(pmc)}→{hex(new_pmc)}, PGRAPH_STATUS={hex(pgraph)}"

def t3_sustained_mixed_storm(bdf):
    """30s sustained mixed read/write storm — ultimate throughput + stability test."""
    deadline = time.time() + 15
    ops_count = 0
    errors = 0
    while time.time() < deadline:
        ops = [
            {"type": "r", "offset": 0x0, "value": 0},
            {"type": "r", "offset": 0x200, "value": 0},
            {"type": "w", "offset": 0x12004C, "value": 0x2},
            {"type": "r", "offset": 0x409100, "value": 0},
            {"type": "r", "offset": 0x87100, "value": 0},
        ]
        try:
            r = rpc(EMBER, "ember.mmio.batch", {"bdf": bdf, "ops": ops}, timeout=10)
            if "error" in r:
                errors += 1
                if errors > 5:
                    return f"too many errors at op {ops_count}"
        except Exception:
            errors += 1
            if errors > 5:
                return f"connection failures at op {ops_count}"
        ops_count += len(ops)
    return f"storm complete: {ops_count} ops, {errors} errors"


# ═══════════════════════════════════════════════════════════════
# TEST REGISTRY
# ═══════════════════════════════════════════════════════════════

VECTORS = {
    # Tier 1 — Safe
    "t1_boot0":        (1, TITAN_V, t1_boot0_read),
    "t1_pmc":          (1, TITAN_V, t1_pmc_read),
    "t1_pri_fault":    (1, TITAN_V, t1_pri_fault_read),
    "t1_pgraph_read":  (1, TITAN_V, t1_pgraph_read),
    "t1_batch_safe":   (1, TITAN_V, t1_batch_safe_reads),
    "t1_rapid_100":    (1, TITAN_V, t1_rapid_fire_reads),
    "t1_batch_pri":    (1, TITAN_V, t1_batch_pri_faults),
    "t1_k80_boot0":    (1, K80_DIE1, t1_boot0_read),
    "t1_k80_pmc":      (1, K80_DIE1, t1_pmc_read),

    # Tier 2 — Dangerous
    "t2_pmc_toggle":       (2, TITAN_V, t2_pmc_toggle),
    "t2_sec2_prepare":     (2, TITAN_V, t2_sec2_prepare),
    "t2_pramin_read":      (2, TITAN_V, t2_pramin_read),
    "t2_pramin_4k":        (2, TITAN_V, t2_pramin_read_4k),
    "t2_pramin_write":     (2, TITAN_V, t2_pramin_write_safe),
    "t2_falcon_start":     (2, TITAN_V, t2_falcon_start_no_fw),
    "t2_falcon_poll":      (2, TITAN_V, t2_falcon_poll_stuck),
    "t2_k80_pmc_toggle":   (2, K80_DIE1, t2_pmc_toggle),

    # Tier 3 — Lethal
    "t3_pramin_bad":         (3, TITAN_V, t3_pramin_bad_addr),
    "t3_pramin_write_bad":   (3, TITAN_V, t3_pramin_write_bad_addr),
    "t3_write_pgraph":       (3, TITAN_V, t3_write_power_gated),
    "t3_write_pfb":          (3, TITAN_V, t3_write_fb_pfb),
    "t3_rapid_writes":       (3, TITAN_V, t3_rapid_fire_writes),
    "t3_batch_mixed":        (3, TITAN_V, t3_batch_dangerous_writes),
    "t3_double_sec2":        (3, TITAN_V, t3_double_sec2_no_warm),
    "t3_k80_pgraph_enable":  (3, K80_DIE1, t3_k80_engine_enable_pgraph),
    "t3_storm":              (3, TITAN_V, t3_sustained_mixed_storm),
}


def warm_cycle(bdf):
    """Warm cycle a device to clean state."""
    log(f"  warm_cycle({bdf})...")
    r = rpc(EMBER, "ember.warm_cycle", {"bdf": bdf}, timeout=60)
    result = r.get("result", r.get("error", {}))
    log(f"  warm_cycle: {result}")
    return "error" not in r


def main():
    tier_filter = sys.argv[1] if len(sys.argv) > 1 else "all"

    log("╔══════════════════════════════════════════════════════════╗")
    log("║  EMBER CONTAINMENT STRESS TEST                          ║")
    log("║  Goal: break ember, not the system                      ║")
    log("╚══════════════════════════════════════════════════════════╝")

    # Pre-flight: warm cycle Titan V
    if ember_alive():
        warm_cycle(TITAN_V)
    else:
        log("FATAL: ember not alive at start")
        sys.exit(1)

    selected = []
    for name, (tier, bdf, fn) in VECTORS.items():
        if tier_filter == "all" or tier_filter == str(tier) or tier_filter == name:
            selected.append((name, tier, bdf, fn))

    log(f"\nRunning {len(selected)} vectors (filter={tier_filter})\n")

    for name, tier, bdf, fn in selected:
        run_vector(name, tier, bdf, fn)

        # Warm cycle Titan V between tier 2+ tests to restore clean state
        if tier >= 2 and bdf == TITAN_V and ember_alive():
            warm_cycle(TITAN_V)

    # Final summary
    log("\n" + "=" * 70)
    log("CONTAINMENT STRESS TEST — FINAL RESULTS")
    log("=" * 70)
    log(f"{'VECTOR':<25} {'TIER':>4} {'BDF':<14} {'STATUS':<25} DETAIL")
    log("-" * 70)

    contained = 0
    sacrificed = 0
    leaked = 0
    for name, tier, bdf, status, detail in results:
        log(f"{name:<25} {tier:>4} {bdf:<14} {status:<25} {detail[:40]}")
        if status == "CONTAINED":
            contained += 1
        elif "SACRIFICED" in status:
            sacrificed += 1
        else:
            leaked += 1

    log("-" * 70)
    log(f"CONTAINED: {contained}  |  SACRIFICED (ember died): {sacrificed}  |  LEAKED: {leaked}")
    log(f"SYSTEM SURVIVED: YES (you're reading this)")
    log("=" * 70)


if __name__ == "__main__":
    main()
