// SPDX-License-Identifier: AGPL-3.0-or-later
//
// local_elementwise_f64.wgsl — airSpring local GPU shaders, f64 canonical.
//
// "Math is universal, precision is silicon" — BarraCuda S67.
//
// Written in f64 (canonical), compiled via compile_shader_universal():
//   F64  → native builtins (Titan V, A100, MI250)
//   Df64 → double-float f32-pair (~48-bit mantissa, consumer GPUs)
//   F32  → downcast_f64_to_f32 (backward compat)
//
// Cross-spring provenance:
//   hotSpring  → math_f64.wgsl precision primitives (S54 pow_f64, acos_f64)
//   wetSpring  → diversity bio shaders (Shannon, Simpson → stats::diversity)
//   groundSpring → MC ET₀ propagation kernel (mc_et0_propagate_f64.wgsl)
//   airSpring  → agricultural science ops below (SCS-CN, ET₀ methods, yield)
//   neuralSpring → compile_shader_universal architecture (S68)
//
// Op 0: SCS-CN runoff       — a=P(mm), b=CN, c=ia_ratio
// Op 1: Stewart yield ratio — a=Ky, b=ETa/ETc, c=unused
// Op 2: Makkink ET₀         — a=T(°C), b=Rs(MJ), c=elev(m)
// Op 3: Turc ET₀            — a=T(°C), b=Rs(MJ), c=RH(%)
// Op 4: Hamon PET           — a=T(°C), b=lat(rad), c=doy
// Op 5: Blaney-Criddle ET₀  — a=T(°C), b=lat(rad), c=doy

enable f64;

struct Params {
    op: u32,
    n: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> in_a: array<f64>;
@group(0) @binding(2) var<storage, read> in_b: array<f64>;
@group(0) @binding(3) var<storage, read> in_c: array<f64>;
@group(0) @binding(4) var<storage, read_write> out: array<f64>;

// Full-precision constants (zero + literal pattern for f64 constant fidelity).
const PI: f64 = f64(0) + 3.14159265358979323846;
const TWO_PI: f64 = f64(0) + 6.28318530717958647692;

fn sat_vp(t: f64) -> f64 {
    // Tetens (1930): es = 0.6108 × exp(17.27 × T / (T + 237.3))
    return (f64(0) + 0.6108) * exp(f64(0) + 17.27 * t / (t + f64(0) + 237.3));
}

fn vp_slope(t: f64) -> f64 {
    let es = sat_vp(t);
    let d = t + f64(0) + 237.3;
    return (f64(0) + 4098.0) * es / (d * d);
}

fn atm_pressure(elev: f64) -> f64 {
    // FAO-56 Eq. 7: P = 101.3 × ((293 - 0.0065z) / 293)^5.26
    return (f64(0) + 101.3) * pow(
        ((f64(0) + 293.0) - (f64(0) + 0.0065) * elev) / (f64(0) + 293.0),
        f64(0) + 5.26
    );
}

fn psychro(p_kpa: f64) -> f64 {
    // FAO-56 Eq. 8: γ = 0.000665 × P
    return (f64(0) + 0.000665) * p_kpa;
}

fn daylight_hr(lat: f64, doy: f64) -> f64 {
    // FAO-56 Eq. 24-25: solar declination → sunset hour angle → daylight hours
    let decl = (f64(0) + 0.4093) * sin(TWO_PI / (f64(0) + 365.0) * doy - (f64(0) + 1.39));
    let arg = clamp(-tan(lat) * tan(decl), f64(0) - 1.0, f64(0) + 1.0);
    let ws = acos(arg);
    return (f64(0) + 24.0) / PI * ws;
}

// ── Op 0: SCS-CN Runoff (USDA TR-55) ────────────────────────────────
fn scs_cn(p: f64, cn: f64, ia_ratio: f64) -> f64 {
    if p <= f64(0) || cn <= f64(0) { return f64(0); }
    let s = (f64(0) + 25400.0) / cn - (f64(0) + 254.0);
    let ia = ia_ratio * s;
    if p <= ia { return f64(0); }
    let pe = p - ia;
    return pe * pe / (pe + s);
}

// ── Op 1: Stewart Yield-Water Function (Doorenbos & Kassam 1979) ─────
fn stewart_yield(ky: f64, eta_etc: f64) -> f64 {
    return f64(0) + 1.0 - ky * (f64(0) + 1.0 - eta_etc);
}

// ── Op 2: Makkink ET₀ (Makkink 1957) ────────────────────────────────
fn makkink(t: f64, rs: f64, elev: f64) -> f64 {
    let p_kpa = atm_pressure(elev);
    let gamma = psychro(p_kpa);
    let delta = vp_slope(t);
    return max(
        (f64(0) + 0.61) * delta / (delta + gamma) * rs / (f64(0) + 2.45) - (f64(0) + 0.12),
        f64(0)
    );
}

// ── Op 3: Turc ET₀ (Turc 1961) ──────────────────────────────────────
fn turc(t: f64, rs: f64, rh: f64) -> f64 {
    let denom = t + f64(0) + 15.0;
    if denom == f64(0) { return f64(0); }
    let t_fac = t / denom;
    if t_fac < f64(0) { return f64(0); }
    let rs_cal = (f64(0) + 23.8846) * rs + f64(0) + 50.0;
    var et0 = (f64(0) + 0.013) * t_fac * rs_cal;
    if rh < f64(0) + 50.0 {
        et0 = et0 * (f64(0) + 1.0 + ((f64(0) + 50.0) - rh) / (f64(0) + 70.0));
    }
    return max(et0, f64(0));
}

// ── Op 4: Hamon PET (Hamon 1963) ─────────────────────────────────────
fn hamon(t: f64, lat: f64, doy: f64) -> f64 {
    if t < f64(0) { return f64(0); }
    let n = daylight_hr(lat, doy);
    if n <= f64(0) { return f64(0); }
    let es = sat_vp(t);
    let rhosat = (f64(0) + 216.7) * es / (t + f64(0) + 273.3);
    return (f64(0) + 0.1651) * n * rhosat;
}

// ── Op 5: Blaney-Criddle ET₀ (Blaney & Criddle 1950) ────────────────
fn blaney_criddle(t: f64, lat: f64, doy: f64) -> f64 {
    let n = daylight_hr(lat, doy);
    let p = n / (f64(0) + 43.80);
    return max(p * ((f64(0) + 0.46) * t + f64(0) + 8.13), f64(0));
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= params.n { return; }

    let a = in_a[idx];
    let b = in_b[idx];
    let c = in_c[idx];

    var result: f64;

    switch params.op {
        case 0u: { result = scs_cn(a, b, c); }
        case 1u: { result = stewart_yield(a, b); }
        case 2u: { result = makkink(a, b, c); }
        case 3u: { result = turc(a, b, c); }
        case 4u: { result = hamon(a, b, c); }
        case 5u: { result = blaney_criddle(a, b, c); }
        default: { result = f64(0); }
    }

    out[idx] = result;
}
