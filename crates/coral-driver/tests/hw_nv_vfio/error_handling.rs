// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::helpers::open_vfio;
use coral_driver::ComputeDevice;

#[cfg(feature = "test-utils")]
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_free_invalid_handle() {
    let mut dev = open_vfio();
    let result = dev.free(coral_driver::BufferHandle::from_id(9999));
    assert!(result.is_err());
}

#[cfg(feature = "test-utils")]
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_readback_invalid_handle() {
    let dev = open_vfio();
    let result = dev.readback(coral_driver::BufferHandle::from_id(9999), 0, 16);
    assert!(result.is_err());
}
