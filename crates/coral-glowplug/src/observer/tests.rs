// SPDX-License-Identifier: AGPL-3.0-only

use coral_ember::observation::{HealthResult, SwapTiming};

use super::nvidia::extract_mmio_addr;
use super::{
    DriverObserver, FindingCategory, NouveauObserver, NvidiaObserver, NvidiaOpenObserver,
    ObserverRegistry, VfioObserver,
};

fn make_obs(personality: &str) -> coral_ember::observation::SwapObservation {
    coral_ember::observation::SwapObservation {
        bdf: "0000:03:00.0".to_string(),
        from_personality: Some("vfio".to_string()),
        to_personality: personality.to_string(),
        timestamp_epoch_ms: 1700000000000,
        timing: SwapTiming {
            prepare_ms: 50,
            unbind_ms: 200,
            bind_ms: 5000,
            stabilize_ms: 100,
            total_ms: 5350,
        },
        trace_path: None,
        health: HealthResult::Ok,
        lifecycle_description: "test".to_string(),
        reset_method_used: None,
    }
}

#[test]
fn nouveau_observer_produces_swap_insight() {
    let obs = NouveauObserver;
    let insight = obs.observe_swap(&make_obs("nouveau")).unwrap();
    assert_eq!(insight.personality, "nouveau");
    assert!(!insight.findings.is_empty());
}

#[test]
fn nouveau_observer_skips_wrong_personality() {
    let obs = NouveauObserver;
    assert!(obs.observe_swap(&make_obs("vfio")).is_none());
}

#[test]
fn vfio_observer_produces_swap_insight() {
    let obs = VfioObserver;
    let insight = obs.observe_swap(&make_obs("vfio")).unwrap();
    assert_eq!(insight.personality, "vfio");
    assert!(insight.findings.len() >= 3);
    assert!(
        insight
            .findings
            .iter()
            .any(|f| f.description.contains("VFIO bind"))
    );
    assert!(
        insight
            .findings
            .iter()
            .any(|f| f.description.contains("handoff type"))
    );
    assert!(
        insight
            .findings
            .iter()
            .any(|f| f.description.contains("VRAM accessible"))
    );
}

#[test]
fn vfio_observer_parses_diagnostic_dump() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dump_path = dir.path().join("vfio_diag.json");
    let dump = serde_json::json!({
        "fecs_cpuctl": 0x00000002_u64,
        "gpccs_cpuctl": 0x00000010_u64,
        "pmc_enable": 0x5fecdff1_u64,
        "vram_alive": true,
        "rings": [
            {"name": "gpfifo", "pending": 0, "fence": 42},
            {"name": "ce0", "pending": 3, "fence": 15}
        ]
    });
    std::fs::write(&dump_path, serde_json::to_string(&dump).unwrap()).unwrap();

    let obs = VfioObserver;
    let insight = obs.observe_trace(dump_path.to_str().unwrap()).unwrap();
    assert_eq!(insight.personality, "vfio");
    assert!(
        insight
            .findings
            .iter()
            .any(|f| f.category == FindingCategory::FalconBoot)
    );
    assert!(
        insight
            .findings
            .iter()
            .any(|f| f.category == FindingCategory::PmcEnable)
    );
    assert!(insight.findings.iter().any(|f| matches!(
        &f.category,
        FindingCategory::Other(s) if s == "ring_health"
    )));
}

#[test]
fn registry_finds_observer_by_personality() {
    let reg = ObserverRegistry::default_observers();
    assert!(reg.for_personality("nouveau").is_some());
    assert!(reg.for_personality("vfio").is_some());
    assert!(reg.for_personality("nvidia").is_some());
    assert!(reg.for_personality("nvidia-open").is_some());
    assert!(reg.for_personality("unknown").is_none());
}

#[test]
fn registry_observe_swap_returns_matching_insights() {
    let reg = ObserverRegistry::default_observers();
    let insights = reg.observe_swap(&make_obs("nouveau"));
    assert_eq!(insights.len(), 1);
    assert_eq!(insights[0].personality, "nouveau");
}

#[test]
fn nouveau_observer_parses_mmiotrace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let trace_path = dir.path().join("test.mmiotrace");
    // Minimal mmiotrace with a PRIV ring reset and a PMC_ENABLE write
    std::fs::write(
        &trace_path,
        "W 4 4 0xf2070000 0x00000001 1 0 0\n\
         W 4 4 0xf2070000 0x00000001 1 0 0\n\
         W 4 4 0xf2000204 0xffffffff 1 0 0\n\
         W 4 4 0xf2100cb8 0x00020000 1 0 0\n",
    )
    .expect("write test trace");

    let obs = NouveauObserver;
    let insight = obs
        .observe_trace(trace_path.to_str().unwrap())
        .expect("parse trace");
    assert_eq!(insight.personality, "nouveau");

    let priv_resets = insight
        .findings
        .iter()
        .find(|f| f.category == FindingCategory::PrivRingReset)
        .expect("priv ring finding");
    assert_eq!(priv_resets.count, Some(2));

    let pmc = insight
        .findings
        .iter()
        .find(|f| f.category == FindingCategory::PmcEnable)
        .expect("pmc finding");
    assert_eq!(pmc.count, Some(1));
}

#[test]
fn nvidia_observer_personality_name() {
    assert_eq!(NvidiaObserver.personality_name(), "nvidia");
    assert_eq!(NvidiaOpenObserver.personality_name(), "nvidia-open");
}

#[test]
fn nvidia_observer_produces_swap_insight() {
    let obs = NvidiaObserver;
    let insight = obs.observe_swap(&make_obs("nvidia")).unwrap();
    assert_eq!(insight.personality, "nvidia");
    assert!(
        insight
            .findings
            .iter()
            .any(|f| f.description.contains("bind completed"))
    );
}

#[test]
fn nvidia_observer_slow_bind_produces_power_state_finding() {
    let mut obs_data = make_obs("nvidia");
    obs_data.timing.total_ms = 10_000;
    let insight = NvidiaObserver.observe_swap(&obs_data).unwrap();
    assert!(
        insight
            .findings
            .iter()
            .any(|f| f.category == FindingCategory::PowerStateChange),
        "slow bind should produce PowerStateChange finding"
    );
}

#[test]
fn nvidia_observer_fast_bind_no_power_state_finding() {
    let mut obs_data = make_obs("nvidia");
    obs_data.timing.total_ms = 500;
    let insight = NvidiaObserver.observe_swap(&obs_data).unwrap();
    assert!(
        !insight
            .findings
            .iter()
            .any(|f| f.category == FindingCategory::PowerStateChange),
        "fast bind should not produce PowerStateChange finding"
    );
}

#[test]
fn nvidia_observer_skips_wrong_personality() {
    assert!(NvidiaObserver.observe_swap(&make_obs("nouveau")).is_none());
}

#[test]
fn nvidia_observer_parses_mmiotrace_with_patterns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let trace_path = dir.path().join("nvidia.mmiotrace");
    std::fs::write(
        &trace_path,
        "W 4 1234 0x00070000 0x00000001 1 0 0\n\
         W 4 1235 0x00070000 0x00000001 1 0 0\n\
         W 4 1236 0x00000200 0xffffffff 1 0 0\n\
         W 4 1237 0x0010a100 0x00000002 1 0 0\n\
         W 4 1238 0x00084004 0x00000040 1 0 0\n\
         R 4 1239 0x00084004 0x00000040 1 0 0\n",
    )
    .expect("write test trace");

    let insight = NvidiaObserver
        .observe_trace(trace_path.to_str().unwrap())
        .expect("parse trace");
    assert_eq!(insight.personality, "nvidia");

    let total = insight
        .findings
        .iter()
        .find(|f| f.description.contains("total MMIO"))
        .unwrap();
    assert_eq!(total.count, Some(5));

    assert!(
        insight
            .findings
            .iter()
            .any(|f| f.category == FindingCategory::PrivRingReset),
        "should detect PRIV ring resets"
    );
    assert!(
        insight
            .findings
            .iter()
            .any(|f| f.category == FindingCategory::FalconBoot),
        "should detect falcon boot writes"
    );
}

#[test]
fn extract_mmio_addr_parses_hex() {
    assert_eq!(
        extract_mmio_addr("W 4 1234 0x00070000 0x00000001 1 0 0"),
        Some(0x0007_0000)
    );
    assert_eq!(
        extract_mmio_addr("R 4 1234 0x00070000 0x00000001 1 0 0"),
        Some(0x0007_0000)
    );
    assert_eq!(extract_mmio_addr("invalid"), None);
}
