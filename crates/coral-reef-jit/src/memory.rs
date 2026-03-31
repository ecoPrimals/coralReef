// SPDX-License-Identifier: AGPL-3.0-only
//! Buffer binding memory layout for JIT execution.
//!
//! Maps `CoralIR` constant buffer (`CBuf`) references to the JIT execution context.
//! In the GPU pipeline, buffer descriptors are placed in constant buffers by the
//! driver. For CPU JIT execution, we provide buffer pointers directly through the
//! `bindings_ptr` function parameter.

use coral_reef_cpu::types::BindingData;

/// Binding buffer manager that owns the backing allocations for JIT execution.
///
/// Each binding's data is copied into a mutable `Vec<u8>` so the JIT kernel can
/// read and write freely. After execution, the modified buffers are extracted back
/// into `BindingData` for the response.
pub struct BindingBuffers {
    buffers: Vec<Vec<u8>>,
}

impl BindingBuffers {
    /// Create binding buffers from the input binding data.
    pub fn from_bindings(bindings: &[BindingData]) -> Self {
        let buffers = bindings.iter().map(|b| b.data.to_vec()).collect();
        Self { buffers }
    }

    /// Get mutable pointers to all buffers for the JIT kernel.
    ///
    /// The returned `Vec` is ordered to match the binding indices. Each pointer
    /// remains valid as long as `self` is not dropped or reallocated.
    #[must_use]
    pub fn as_mut_ptrs(&mut self) -> Vec<*mut u8> {
        self.buffers.iter_mut().map(Vec::as_mut_ptr).collect()
    }

    /// Extract modified buffer data back into `BindingData` format.
    ///
    /// Consumes the buffer manager, transferring ownership of the backing
    /// allocations into `bytes::Bytes` for zero-copy IPC forwarding.
    pub fn into_binding_data(self, original: &[BindingData]) -> Vec<BindingData> {
        self.buffers
            .into_iter()
            .zip(original.iter())
            .map(|(buf, orig)| BindingData {
                group: orig.group,
                binding: orig.binding,
                data: bytes::Bytes::from(buf),
                usage: orig.usage,
            })
            .collect()
    }

    /// Number of buffers.
    #[must_use]
    pub fn count(&self) -> usize {
        self.buffers.len()
    }

    /// Get a reference to a specific buffer by index.
    #[must_use]
    pub fn buffer(&self, index: usize) -> Option<&[u8]> {
        self.buffers.get(index).map(Vec::as_slice)
    }

    /// Get a mutable reference to a specific buffer by index.
    pub fn buffer_mut(&mut self, index: usize) -> Option<&mut [u8]> {
        self.buffers.get_mut(index).map(Vec::as_mut_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use coral_reef_cpu::types::BindingUsage;

    #[test]
    fn round_trip_preserves_data() {
        let bindings = vec![
            BindingData {
                group: 0,
                binding: 0,
                data: Bytes::from_static(&[1, 2, 3, 4]),
                usage: BindingUsage::ReadOnly,
            },
            BindingData {
                group: 0,
                binding: 1,
                data: Bytes::from_static(&[0, 0, 0, 0]),
                usage: BindingUsage::ReadWrite,
            },
        ];

        let mut bufs = BindingBuffers::from_bindings(&bindings);
        assert_eq!(bufs.count(), 2);
        assert_eq!(bufs.buffer(0), Some([1u8, 2, 3, 4].as_slice()));

        if let Some(b) = bufs.buffer_mut(1) {
            b.copy_from_slice(&[5, 6, 7, 8]);
        }

        let out = bufs.into_binding_data(&bindings);
        assert_eq!(out[0].data.as_ref(), &[1, 2, 3, 4]);
        assert_eq!(out[1].data.as_ref(), &[5, 6, 7, 8]);
        assert_eq!(out[0].group, 0);
        assert_eq!(out[1].binding, 1);
    }

    #[test]
    fn empty_bindings() {
        let bufs = BindingBuffers::from_bindings(&[]);
        assert_eq!(bufs.count(), 0);
        assert!(bufs.buffer(0).is_none());
    }
}
