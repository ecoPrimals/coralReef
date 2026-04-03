// SPDX-License-Identifier: AGPL-3.0-only
//! Built-in shader variables.

/// Built-in shader input/output variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltIn {
    GlobalInvocationId,
    LocalInvocationId,
    LocalInvocationIndex,
    WorkGroupId,
    WorkGroupSize,
    NumWorkGroups,
    VertexIndex,
    InstanceIndex,
    Position,
    FrontFacing,
    FragDepth,
    SampleIndex,
    SampleMask,
    SubgroupId,
    SubgroupSize,
    SubgroupInvocationId,
}
