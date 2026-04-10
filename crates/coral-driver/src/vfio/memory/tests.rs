// SPDX-License-Identifier: AGPL-3.0-or-later

use super::regions::pramin_window_layout;
use super::*;

/// Same sentinels and equality rule as `nv::vfio_compute::acr_boot::strategy_sysmem`
/// `attempt_sysmem_acr_boot_inner` step 0 (PRAMIN write–readback VRAM probe).
fn pramin_vram_roundtrip_ok(wrote: (u32, u32), read_back: (u32, u32)) -> bool {
    read_back.0 == wrote.0 && read_back.1 == wrote.1
}

#[test]
fn pramin_vram_roundtrip_sysmem_impl_pattern() {
    const S1: u32 = 0xACB0_1234;
    const S2: u32 = 0xFEED_FACE;
    assert!(pramin_vram_roundtrip_ok((S1, S2), (S1, S2)));
    assert!(!pramin_vram_roundtrip_ok((S1, S2), (0, S2)));
    assert!(!pramin_vram_roundtrip_ok((S1, S2), (S1, 0)));
    assert!(!pramin_vram_roundtrip_ok((S1, S2), (S2, S1)));
}

#[test]
fn path_status_sentinel_match() {
    let status = PathStatus::from_sentinel_test(0xDEAD_BEEF, 0xDEAD_BEEF, 42);
    assert!(status.is_working());
    assert_eq!(status, PathStatus::Working { latency_us: 42 });
}

#[test]
fn path_status_sentinel_bad0() {
    let status = PathStatus::from_sentinel_test(0xDEAD_BEEF, 0xBAD0_AC00, 0);
    assert!(status.is_error_pattern());
}

#[test]
fn path_status_sentinel_ffff() {
    let status = PathStatus::from_sentinel_test(0xDEAD_BEEF, 0xFFFF_FFFF, 0);
    assert!(status.is_error_pattern());
}

#[test]
fn path_status_sentinel_corrupt() {
    let status = PathStatus::from_sentinel_test(0xDEAD_BEEF, 0x1234_5678, 0);
    assert!(!status.is_working());
    assert!(!status.is_error_pattern());
    assert_eq!(
        status,
        PathStatus::Corrupted {
            wrote: 0xDEAD_BEEF,
            read: 0x1234_5678
        }
    );
}

#[test]
fn aperture_display() {
    let a = Aperture::SystemMemory {
        iova: 0x1000,
        coherent: true,
    };
    assert!(format!("{a}").contains("0x1000"));

    let b = Aperture::VideoMemory {
        vram_offset: 0x20000,
    };
    assert!(format!("{b}").contains("VRAM"));

    let c = Aperture::RegisterSpace { offset: 0x200 };
    assert!(format!("{c}").contains("MMIO"));
}

#[test]
fn memory_error_display() {
    let e = MemoryError::OutOfBounds {
        offset: 0x100,
        size: 0x80,
    };
    assert!(format!("{e}").contains("out of bounds"));
}

#[test]
fn memory_topology_working_paths_empty() {
    let topo = MemoryTopology {
        vram_accessible: false,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![],
        evidence: vec![],
    };
    assert!(topo.working_paths(PathMethod::Pramin).is_empty());
    assert!(!topo.pramin_works());
    assert!(!topo.dma_works());
}

#[test]
fn memory_delta_compute_gains() {
    let before = MemoryTopology {
        vram_accessible: false,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![AccessPath {
            from: "cpu",
            to: Aperture::VideoMemory { vram_offset: 0 },
            method: PathMethod::Pramin,
            status: PathStatus::ErrorPattern {
                pattern: 0xBAD0_AC00,
            },
            prerequisites: vec![],
        }],
        evidence: vec![],
    };

    let after = MemoryTopology {
        vram_accessible: true,
        vram_size_probed: 0x1000,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![AccessPath {
            from: "cpu",
            to: Aperture::VideoMemory { vram_offset: 0 },
            method: PathMethod::Pramin,
            status: PathStatus::Working { latency_us: 5 },
            prerequisites: vec![],
        }],
        evidence: vec![],
    };

    let delta = MemoryDelta::compute((0x200, 0xFFFF_FFFF), before, after);
    assert!(delta.unlocked_memory());
    assert!(!delta.broke_memory());
    assert_eq!(delta.paths_gained.len(), 1);
    assert_eq!(delta.paths_gained[0].method, PathMethod::Pramin);
}

#[test]
fn pramin_window_layout_ok() {
    let (base, off) = pramin_window_layout(0x12_3456, 0x100).expect("layout");
    assert_eq!(base, 0x12_0000);
    assert_eq!(off, 0x3456);
}

#[test]
fn pramin_window_layout_spans_boundary_err() {
    let err = pramin_window_layout(0xFFFF_0000, 0x2_0000).expect_err("expected boundary error");
    let msg = format!("{err}");
    assert!(msg.contains("window boundary"), "{msg}");
}

#[test]
fn memory_error_not_accessible_display() {
    let e = MemoryError::NotAccessible {
        reason: "test".to_string(),
    };
    assert!(format!("{e}").contains("not accessible"));

    let io = MemoryError::IoError {
        detail: "e".to_string(),
    };
    assert!(format!("{io}").contains("I/O"));
}

#[test]
fn path_status_methods() {
    assert!(!PathStatus::Untested.is_working());
    assert!(!PathStatus::Untested.is_error_pattern());
}

#[test]
fn memory_topology_helpers() {
    let topo = MemoryTopology {
        vram_accessible: true,
        vram_size_probed: 0x1000,
        sysmem_dma_ok: true,
        bar2_configured: false,
        paths: vec![
            AccessPath {
                from: "cpu",
                to: Aperture::SystemMemory {
                    iova: 0x1000,
                    coherent: true,
                },
                method: PathMethod::DmaCoherent,
                status: PathStatus::Working { latency_us: 1 },
                prerequisites: vec![],
            },
            AccessPath {
                from: "cpu",
                to: Aperture::SystemMemory {
                    iova: 0x2000,
                    coherent: false,
                },
                method: PathMethod::DmaNonCoherent,
                status: PathStatus::Working { latency_us: 2 },
                prerequisites: vec![],
            },
        ],
        evidence: vec![],
    };
    assert!(topo.dma_works());
    assert_eq!(topo.working_paths(PathMethod::DmaCoherent).len(), 1);
}

#[test]
fn memory_delta_lost_paths() {
    let before = MemoryTopology {
        vram_accessible: true,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![AccessPath {
            from: "cpu",
            to: Aperture::VideoMemory { vram_offset: 0 },
            method: PathMethod::Pramin,
            status: PathStatus::Working { latency_us: 1 },
            prerequisites: vec![],
        }],
        evidence: vec![],
    };
    let after = MemoryTopology {
        vram_accessible: false,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![AccessPath {
            from: "cpu",
            to: Aperture::VideoMemory { vram_offset: 0 },
            method: PathMethod::Pramin,
            status: PathStatus::ErrorPattern {
                pattern: 0xFFFF_FFFF,
            },
            prerequisites: vec![],
        }],
        evidence: vec![],
    };
    let d = MemoryDelta::compute((0, 0), before, after);
    assert!(d.broke_memory());
    assert_eq!(d.paths_lost.len(), 1);
}

#[test]
fn path_status_bad0_prefix_edge_cases() {
    let bad0_err_reads = [0xBAD0_0000_u32, 0xBAD0_FFFF];
    for read in bad0_err_reads {
        let s = PathStatus::from_sentinel_test(0x1111_1111, read, 0);
        assert!(
            s.is_error_pattern(),
            "{read:#x} expected BAD0 error pattern"
        );
    }
    // `from_sentinel_test` treats `(read >> 16) == 0xBAD0` as GPU error; 0xBADF is distinct.
    let badf_corrupt_reads = [0xBADF_0000_u32, 0xBADF_FFFF];
    for read in badf_corrupt_reads {
        let s = PathStatus::from_sentinel_test(0x1111_1111, read, 0);
        assert!(
            !s.is_error_pattern(),
            "{read:#x} should not classify as BAD0 error tag"
        );
        assert!(matches!(s, PathStatus::Corrupted { .. }));
    }
    let not_err = [0xBAD1_0000_u32, 0x0000_BAD0];
    for read in not_err {
        let s = PathStatus::from_sentinel_test(0x1111_1111, read, 0);
        assert!(
            !s.is_error_pattern(),
            "{read:#x} should not classify as GPU error prefix"
        );
        assert!(
            matches!(s, PathStatus::Corrupted { .. }),
            "{read:#x} expected corrupted",
            read = read
        );
    }
}

#[test]
fn memory_delta_same_path_different_latency_no_net_change() {
    let path = AccessPath {
        from: "cpu",
        to: Aperture::VideoMemory {
            vram_offset: 0x1000,
        },
        method: PathMethod::Pramin,
        status: PathStatus::Working { latency_us: 1 },
        prerequisites: vec![],
    };
    let before = MemoryTopology {
        vram_accessible: true,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![path.clone()],
        evidence: vec![],
    };
    let mut after_path = path;
    after_path.status = PathStatus::Working { latency_us: 99 };
    let after = MemoryTopology {
        vram_accessible: true,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![after_path],
        evidence: vec![],
    };
    let d = MemoryDelta::compute((0x10, 0), before, after);
    assert!(!d.unlocked_memory());
    assert!(!d.broke_memory());
    assert!(d.paths_gained.is_empty());
    assert!(d.paths_lost.is_empty());
}

#[test]
fn memory_delta_same_key_non_working_to_non_working() {
    let p = |status: PathStatus| AccessPath {
        from: "cpu",
        to: Aperture::VideoMemory { vram_offset: 0 },
        method: PathMethod::Pramin,
        status,
        prerequisites: vec![],
    };
    let before = MemoryTopology {
        vram_accessible: false,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![p(PathStatus::ErrorPattern {
            pattern: 0xBAD0_0000,
        })],
        evidence: vec![],
    };
    let after = MemoryTopology {
        vram_accessible: false,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![p(PathStatus::Corrupted { wrote: 1, read: 2 })],
        evidence: vec![],
    };
    let d = MemoryDelta::compute((0, 0), before, after);
    assert!(d.paths_gained.is_empty());
    assert!(d.paths_lost.is_empty());
}

#[test]
fn memory_delta_path_only_in_before_lost() {
    let before = MemoryTopology {
        vram_accessible: true,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![AccessPath {
            from: "cpu",
            to: Aperture::VideoMemory {
                vram_offset: 0xABCD,
            },
            method: PathMethod::Pramin,
            status: PathStatus::Working { latency_us: 1 },
            prerequisites: vec![],
        }],
        evidence: vec![],
    };
    let after = MemoryTopology {
        vram_accessible: false,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![],
        evidence: vec![],
    };
    let d = MemoryDelta::compute((0, 0), before, after);
    assert_eq!(d.paths_lost.len(), 1);
    assert!(d.paths_gained.is_empty());
}

#[test]
fn memory_delta_path_only_in_after_gained() {
    let after = MemoryTopology {
        vram_accessible: true,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![AccessPath {
            from: "cpu",
            to: Aperture::VideoMemory {
                vram_offset: 0xDCBA,
            },
            method: PathMethod::Bar1,
            status: PathStatus::Working { latency_us: 3 },
            prerequisites: vec![],
        }],
        evidence: vec![],
    };
    let before = MemoryTopology {
        vram_accessible: false,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![],
        evidence: vec![],
    };
    let d = MemoryDelta::compute((0, 0), before, after);
    assert_eq!(d.paths_gained.len(), 1);
    assert!(d.paths_lost.is_empty());
}

#[test]
fn memory_delta_both_empty_topologies() {
    let empty = || MemoryTopology {
        vram_accessible: false,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: vec![],
        evidence: vec![],
    };
    let d = MemoryDelta::compute((0x1700, 0x1), empty(), empty());
    assert!(d.paths_gained.is_empty());
    assert!(d.paths_lost.is_empty());
}

#[test]
fn memory_delta_large_topology_all_accounted() {
    const N: u32 = 48;
    let mut paths_before = Vec::new();
    let mut paths_after = Vec::new();
    for i in 0..N {
        let off = u64::from(i) * 0x1000;
        paths_before.push(AccessPath {
            from: "cpu",
            to: Aperture::VideoMemory { vram_offset: off },
            method: PathMethod::Pramin,
            status: PathStatus::Working {
                latency_us: u64::from(i),
            },
            prerequisites: vec![],
        });
        let status = if i % 3 == 0 {
            PathStatus::ErrorPattern {
                pattern: 0xFFFF_FFFF,
            }
        } else {
            PathStatus::Working {
                latency_us: u64::from(i) + 10,
            }
        };
        paths_after.push(AccessPath {
            from: "cpu",
            to: Aperture::VideoMemory { vram_offset: off },
            method: PathMethod::Pramin,
            status,
            prerequisites: vec![],
        });
    }
    let before = MemoryTopology {
        vram_accessible: true,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: paths_before,
        evidence: vec![],
    };
    let after = MemoryTopology {
        vram_accessible: true,
        vram_size_probed: 0,
        sysmem_dma_ok: false,
        bar2_configured: false,
        paths: paths_after,
        evidence: vec![],
    };
    let d = MemoryDelta::compute((0, 0), before, after);
    let lost_expected = (0..N).filter(|i| i % 3 == 0).count();
    let gained_expected = 0_usize;
    assert_eq!(d.paths_lost.len(), lost_expected);
    assert_eq!(d.paths_gained.len(), gained_expected);
}

#[test]
fn pramin_window_layout_one_mib_boundary_and_zero() {
    let (base, off) = pramin_window_layout(0x0010_0000, 4).expect("1 MiB aligned");
    assert_eq!(base, 0x0010_0000);
    assert_eq!(off, 0);
    let (zbase, zoff) = pramin_window_layout(0, 0x40).expect("zero base");
    assert_eq!(zbase, 0);
    assert_eq!(zoff, 0);
}

#[test]
fn pramin_window_layout_end_of_64k_window() {
    let (base, off) = pramin_window_layout(0x12_FFFC, 4).expect("last u32 in window");
    assert_eq!(base, 0x12_0000);
    assert_eq!(off, 0xFFFC);
    let err = pramin_window_layout(0x12_FFFC, 8).expect_err("spans past window end");
    assert!(format!("{err}").contains("window boundary"));
}
