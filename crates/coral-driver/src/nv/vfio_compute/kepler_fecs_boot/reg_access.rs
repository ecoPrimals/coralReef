// SPDX-License-Identifier: AGPL-3.0-or-later

/// `RegisterAccess` adapter routing through `GuardedBar` — writes go through
/// the blocklist/canary checks, reads through the link-alive check.
pub(super) struct GuardedBarRegAccess<'a>(
    pub(super) &'a super::super::hardware_guard::GuardedBar<'a>,
);

impl crate::gsp::RegisterAccess for GuardedBarRegAccess<'_> {
    fn read_u32(&self, offset: u32) -> Result<u32, crate::gsp::ApplyError> {
        self.0
            .read_u32(offset)
            .map_err(|refusal| crate::gsp::ApplyError::MmioFailed {
                offset,
                detail: refusal.to_string(),
            })
    }

    fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), crate::gsp::ApplyError> {
        self.0
            .write_u32(offset, value)
            .map_err(|refusal| crate::gsp::ApplyError::MmioFailed {
                offset,
                detail: refusal.to_string(),
            })
    }
}
