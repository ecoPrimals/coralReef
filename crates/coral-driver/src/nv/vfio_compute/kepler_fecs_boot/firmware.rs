// SPDX-License-Identifier: AGPL-3.0-or-later
//! GK110/GK210 FECS/GPCCS firmware blob discovery — internal vs external search order.

/// Resolved firmware payloads for cold-path Kepler FECS/GPCCS boot.
pub(super) struct KeplerFirmwareBlobs {
    pub(super) fecs_code: Vec<u8>,
    pub(super) fecs_data: Vec<u8>,
    pub(super) gpccs_code: Vec<u8>,
    pub(super) gpccs_data: Vec<u8>,
    pub(super) use_internal_protocol: bool,
}

/// Locate the best available firmware directory and blobs (`None` when nothing usable loads).
#[must_use]
pub(super) fn resolve_kepler_firmware() -> Option<KeplerFirmwareBlobs> {
    // K80 is GK210B (GK110B die). We now prefer EXTERNAL firmware because
    // empirical testing shows both FECS and GPCCS start successfully via host
    // MMIO STARTCPU immediately after firmware PIO upload. External firmware
    // (/lib/firmware/nvidia/gk210/, ~15KB) is self-configuring and doesn't
    // need csdata in DMEM. Internal firmware (nouveau.ko embedded, ~3KB)
    // requires csdata and a different boot protocol.
    //
    // Priority: internal (FECS starts GPCCS via DMA — required on GK210B where
    // host MMIO STARTCPU is silently ignored for per-GPC GPCCS falcons) →
    // external → system → gk110 fallback
    let system_gk210 = crate::linux_paths::nvidia_firmware_path("gk210", "");
    let fw_search: &[(&str, &str, &str, &str, &str, bool)] = &[
        (
            "gk110-internal",
            concat!(env!("CARGO_MANIFEST_DIR"), "/firmware/gk210"),
            "gk110_internal_fecs_code.bin",
            "gk110_internal_fecs_data.bin",
            "gk110_internal_gpccs_code.bin",
            true,
        ),
        (
            "gk210-external",
            concat!(env!("CARGO_MANIFEST_DIR"), "/firmware/gk210"),
            "gk210_fecs_code.bin",
            "gk210_fecs_data.bin",
            "gk210_gpccs_code.bin",
            false,
        ),
        (
            "gk210-system",
            &system_gk210,
            "fecs_inst.bin",
            "fecs_data.bin",
            "gpccs_inst.bin",
            false,
        ),
        (
            "gk110-fallback",
            concat!(env!("CARGO_MANIFEST_DIR"), "/firmware/gk110"),
            "gk110_fecs_code.bin",
            "gk110_fecs_data.bin",
            "gk110_gpccs_code.bin",
            false,
        ),
    ];

    let mut fecs_code = None;
    let mut fecs_data = None;
    let mut gpccs_code = None;
    let mut gpccs_data = None;
    let mut use_internal_protocol = false;

    const MIN_FECS_CODE_BYTES: usize = 8192;
    const MIN_GPCCS_CODE_BYTES: usize = 4096;

    for &(label, dir, fc, fd, gc, is_internal) in fw_search {
        let gd = fc
            .replace("fecs", "gpccs")
            .replace("inst", "data")
            .replace("code", "data");
        let gd_name = if std::fs::metadata(format!("{dir}/{gd}")).is_ok() {
            gd.clone()
        } else {
            gc.replace("inst", "data").replace("code", "data")
        };

        let try_read = |name: &str| -> Option<Vec<u8>> {
            let path = format!("{dir}/{name}");
            std::fs::read(&path).ok().inspect(|data| {
                tracing::info!(path, bytes = data.len(), label, "loaded firmware");
            })
        };

        if let (Some(fc_data), Some(fd_data), Some(gc_data), Some(gd_data)) =
            (try_read(fc), try_read(fd), try_read(gc), try_read(&gd_name))
        {
            if !is_internal
                && (fc_data.len() < MIN_FECS_CODE_BYTES || gc_data.len() < MIN_GPCCS_CODE_BYTES)
            {
                tracing::warn!(
                    label,
                    fecs_code = fc_data.len(),
                    gpccs_code = gc_data.len(),
                    min_fecs = MIN_FECS_CODE_BYTES,
                    min_gpccs = MIN_GPCCS_CODE_BYTES,
                    "firmware set rejected — code blobs too small (likely truncated capture)"
                );
                continue;
            }
            tracing::info!(
                label,
                is_internal,
                fecs_code = fc_data.len(),
                fecs_data = fd_data.len(),
                gpccs_code = gc_data.len(),
                gpccs_data = gd_data.len(),
                "Selected firmware set"
            );
            fecs_code = Some(fc_data);
            fecs_data = Some(fd_data);
            gpccs_code = Some(gc_data);
            gpccs_data = Some(gd_data);
            use_internal_protocol = is_internal;
            break;
        }
        tracing::debug!(label, dir, "firmware set not complete, trying next");
    }

    let (Some(fecs_code), Some(fecs_data), Some(gpccs_code), Some(gpccs_data)) =
        (fecs_code, fecs_data, gpccs_code, gpccs_data)
    else {
        tracing::warn!("No GK210/GK110 FECS/GPCCS firmware available — GR will not work");
        return None;
    };

    Some(KeplerFirmwareBlobs {
        fecs_code,
        fecs_data,
        gpccs_code,
        gpccs_data,
        use_internal_protocol,
    })
}
