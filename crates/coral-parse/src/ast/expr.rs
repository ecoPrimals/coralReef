// SPDX-License-Identifier: AGPL-3.0-only
//! Expression AST nodes.

use super::{Handle, Type};

/// An expression in the AST. Stored in an arena, referenced by `Handle<Expression>`.
#[derive(Debug, Clone)]
pub enum Expression {
    /// A literal constant value.
    Literal(Literal),

    /// Construct a zero value of the given type.
    ZeroValue(Handle<Type>),

    /// Binary operation: `left op right`.
    Binary {
        op: BinaryOp,
        left: Handle<Expression>,
        right: Handle<Expression>,
    },

    /// Unary operation: `op expr`.
    Unary {
        op: UnaryOp,
        expr: Handle<Expression>,
    },

    /// Built-in math function call.
    Math {
        fun: MathFunction,
        arg: Handle<Expression>,
        arg1: Option<Handle<Expression>>,
        arg2: Option<Handle<Expression>>,
    },

    /// Ternary select: `select(reject, accept, condition)`.
    Select {
        condition: Handle<Expression>,
        accept: Handle<Expression>,
        reject: Handle<Expression>,
    },

    /// Reference to a function argument (by index).
    FunctionArgument(u32),

    /// Reference to a global variable (by index into `Module::global_variables`).
    GlobalVariable(u32),

    /// Reference to a local variable (by index into `Function::local_variables`).
    LocalVariable(u32),

    /// Load a value from a pointer expression.
    Load { pointer: Handle<Expression> },

    /// Dynamic array/vector index: `base[index]`.
    Access {
        base: Handle<Expression>,
        index: Handle<Expression>,
    },

    /// Static struct member or vector component access: `base.field`.
    AccessIndex {
        base: Handle<Expression>,
        index: u32,
    },

    /// Compose a composite value from components.
    Compose {
        ty: Handle<Type>,
        components: Vec<Handle<Expression>>,
    },

    /// Splat a scalar to a vector.
    Splat {
        size: super::VectorSize,
        value: Handle<Expression>,
    },

    /// Swizzle vector components.
    Swizzle {
        vector: Handle<Expression>,
        pattern: [u32; 4],
        size: super::VectorSize,
    },

    /// Type cast (as<T>).
    As {
        expr: Handle<Expression>,
        kind: super::ScalarKind,
        convert: Option<u8>,
    },

    /// Query the length of a runtime-sized array.
    ArrayLength(Handle<Expression>),

    /// A named constant reference (for `const` declarations).
    Constant(Handle<Expression>),

    /// Texture sample: `textureSample(texture, sampler, coord)`.
    TextureSample {
        texture: Handle<Expression>,
        sampler: Handle<Expression>,
        coordinate: Handle<Expression>,
        array_index: Option<Handle<Expression>>,
        offset: Option<Handle<Expression>>,
    },

    /// Texture sample with explicit level: `textureSampleLevel(texture, sampler, coord, level)`.
    TextureSampleLevel {
        texture: Handle<Expression>,
        sampler: Handle<Expression>,
        coordinate: Handle<Expression>,
        level: Handle<Expression>,
        array_index: Option<Handle<Expression>>,
        offset: Option<Handle<Expression>>,
    },

    /// Texture sample with bias: `textureSampleBias(texture, sampler, coord, bias)`.
    TextureSampleBias {
        texture: Handle<Expression>,
        sampler: Handle<Expression>,
        coordinate: Handle<Expression>,
        bias: Handle<Expression>,
        array_index: Option<Handle<Expression>>,
        offset: Option<Handle<Expression>>,
    },

    /// Texture comparison sample: `textureSampleCompare(texture, sampler, coord, ref)`.
    TextureSampleCompare {
        texture: Handle<Expression>,
        sampler: Handle<Expression>,
        coordinate: Handle<Expression>,
        depth_ref: Handle<Expression>,
        array_index: Option<Handle<Expression>>,
        offset: Option<Handle<Expression>>,
    },

    /// Load a single texel: `textureLoad(texture, coord, level)`.
    TextureLoad {
        texture: Handle<Expression>,
        coordinate: Handle<Expression>,
        array_index: Option<Handle<Expression>>,
        level: Option<Handle<Expression>>,
        sample_index: Option<Handle<Expression>>,
    },

    /// Query texture dimensions: `textureDimensions(texture, level)`.
    TextureDimensions {
        texture: Handle<Expression>,
        level: Option<Handle<Expression>>,
    },

    /// Query number of texture array layers.
    TextureNumLayers {
        texture: Handle<Expression>,
    },

    /// Query number of mip levels.
    TextureNumLevels {
        texture: Handle<Expression>,
    },

    /// Query number of multisampled samples.
    TextureNumSamples {
        texture: Handle<Expression>,
    },
}

/// Literal values.
#[derive(Debug, Clone, Copy)]
pub enum Literal {
    F32(f32),
    F64(f64),
    U32(u32),
    I32(i32),
    Bool(bool),
}

/// Binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
    BitwiseAnd,
    BitwiseOr,
    BitwiseXor,
    ShiftLeft,
    ShiftRight,
}

/// Unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    BitwiseNot,
    LogicalNot,
}

/// Built-in math functions (maps to hardware transcendentals and ALU ops).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathFunction {
    Abs,
    Min,
    Max,
    Clamp,
    Floor,
    Ceil,
    Round,
    Trunc,
    Fract,
    Sqrt,
    InverseSqrt,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Exp,
    Exp2,
    Log,
    Log2,
    Pow,
    Dot,
    Cross,
    Normalize,
    Length,
    Distance,
    Fma,
    Mix,
    Step,
    SmoothStep,
    Sign,
    CountOneBits,
    ReverseBits,
    FirstLeadingBit,
    FirstTrailingBit,
    ExtractBits,
    InsertBits,
}
