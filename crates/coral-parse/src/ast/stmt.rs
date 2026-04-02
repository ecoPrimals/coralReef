// SPDX-License-Identifier: AGPL-3.0-only
//! Statement AST nodes.

use super::{Expression, Handle};

/// A statement in the AST.
#[derive(Debug, Clone)]
pub enum Statement {
    /// Emit expressions (make them available for use).
    Emit(std::ops::Range<Handle<Expression>>),

    /// Store a value to a pointer.
    Store {
        pointer: Handle<Expression>,
        value: Handle<Expression>,
    },

    /// If/else conditional.
    If {
        condition: Handle<Expression>,
        accept: Vec<Statement>,
        reject: Vec<Statement>,
    },

    /// Loop with body and continuing block.
    Loop {
        body: Vec<Statement>,
        continuing: Vec<Statement>,
        break_if: Option<Handle<Expression>>,
    },

    /// Switch statement.
    Switch {
        selector: Handle<Expression>,
        cases: Vec<SwitchCase>,
    },

    /// Return from the function.
    Return { value: Option<Handle<Expression>> },

    /// Break out of the innermost loop.
    Break,

    /// Continue to the next iteration.
    Continue,

    /// Nested block of statements.
    Block(Vec<Statement>),

    /// Workgroup/storage barrier.
    ControlBarrier(Barrier),

    /// Memory barrier only (no execution sync).
    MemoryBarrier(Barrier),

    /// Kill the invocation (discard in fragment, illegal in compute).
    Kill,

    /// Call a function.
    Call {
        function: u32,
        arguments: Vec<Handle<Expression>>,
        result: Option<Handle<Expression>>,
    },

    /// Atomic operation.
    Atomic {
        pointer: Handle<Expression>,
        fun: AtomicFunction,
        value: Handle<Expression>,
        result: Option<Handle<Expression>>,
    },

    /// A `for` loop (desugared to `Loop` during lowering, but useful for AST fidelity).
    ForLoop {
        init: Option<Box<Statement>>,
        condition: Option<Handle<Expression>>,
        update: Option<Box<Statement>>,
        body: Vec<Statement>,
    },

    /// A `while` loop (syntactic sugar).
    WhileLoop {
        condition: Handle<Expression>,
        body: Vec<Statement>,
    },

    /// Local variable declaration with optional initializer (compound statement).
    LocalDecl {
        local_var_index: u32,
    },

    /// Phony assignment: `_ = expr;`
    Phony { value: Handle<Expression> },

    /// Increment: `*ptr += 1`.
    Increment { pointer: Handle<Expression> },

    /// Decrement: `*ptr -= 1`.
    Decrement { pointer: Handle<Expression> },

    /// Compound assignment: `*ptr op= value`.
    CompoundAssign {
        pointer: Handle<Expression>,
        op: super::BinaryOp,
        value: Handle<Expression>,
    },

    /// `textureStore(texture, coord, value)`.
    TextureStore {
        texture: Handle<Expression>,
        coordinate: Handle<Expression>,
        array_index: Option<Handle<Expression>>,
        value: Handle<Expression>,
    },
}

/// A case in a switch statement.
#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub value: SwitchValue,
    pub body: Vec<Statement>,
    pub fall_through: bool,
}

/// Value matched in a switch case.
#[derive(Debug, Clone, Copy)]
pub enum SwitchValue {
    I32(i32),
    U32(u32),
    Default,
}

/// Barrier flags.
#[derive(Debug, Clone, Copy)]
pub struct Barrier {
    pub workgroup: bool,
    pub storage: bool,
}

impl Barrier {
    pub const WORK_GROUP: Self = Self { workgroup: true, storage: false };
    pub const STORAGE: Self = Self { workgroup: false, storage: true };
    pub const ALL: Self = Self { workgroup: true, storage: true };
}

/// Atomic operation kind.
#[derive(Debug, Clone, Copy)]
pub enum AtomicFunction {
    Add,
    Subtract,
    Min,
    Max,
    And,
    Or,
    Xor,
    Exchange,
    CompareExchange,
}
