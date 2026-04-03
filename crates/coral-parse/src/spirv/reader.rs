// SPDX-License-Identifier: AGPL-3.0-only
//! SPIR-V binary reader — two-pass parse into [`crate::ast::Module`].

use crate::ast::*;
use crate::error::ParseError;
use coral_reef_stubs::fxhash::FxHashMap;

const SPIRV_MAGIC: u32 = 0x0723_0203;

// Opcodes (subset; extend as needed)
const OP_EXT_INST_IMPORT: u16 = 11;
const OP_MEMORY_MODEL: u16 = 14;
const OP_ENTRY_POINT: u16 = 15;
const OP_EXECUTION_MODE: u16 = 16;
const OP_CAPABILITY: u16 = 17;
const OP_TYPE_VOID: u16 = 19;
const OP_TYPE_BOOL: u16 = 20;
const OP_TYPE_INT: u16 = 21;
const OP_TYPE_FLOAT: u16 = 22;
const OP_TYPE_VECTOR: u16 = 23;
const OP_TYPE_MATRIX: u16 = 24;
const OP_TYPE_IMAGE: u16 = 25;
const OP_TYPE_SAMPLER: u16 = 26;
const OP_TYPE_SAMPLED_IMAGE: u16 = 27;
const OP_TYPE_ARRAY: u16 = 28;
const OP_TYPE_RUNTIME_ARRAY: u16 = 29;
const OP_TYPE_STRUCT: u16 = 30;
const OP_TYPE_POINTER: u16 = 32;
const OP_TYPE_FUNCTION: u16 = 33;
const OP_CONSTANT_TRUE: u16 = 41;
const OP_CONSTANT_FALSE: u16 = 42;
const OP_CONSTANT: u16 = 43;
const OP_CONSTANT_COMPOSITE: u16 = 44;
const OP_FUNCTION: u16 = 54;
const OP_FUNCTION_PARAMETER: u16 = 55;
const OP_FUNCTION_END: u16 = 56;
const OP_VARIABLE: u16 = 59;
const OP_LOAD: u16 = 61;
const OP_STORE: u16 = 62;
const OP_ACCESS_CHAIN: u16 = 65;
const OP_IN_BOUNDS_ACCESS_CHAIN: u16 = 66;
const OP_COMPOSITE_CONSTRUCT: u16 = 80;
const OP_COMPOSITE_EXTRACT: u16 = 81;
const OP_EXT_INST: u16 = 12;
const OP_BITCAST: u16 = 124;
const OP_FNEGATE: u16 = 127;
const OP_IADD: u16 = 128;
const OP_FADD: u16 = 129;
const OP_ISUB: u16 = 130;
const OP_FSUB: u16 = 131;
const OP_IMUL: u16 = 132;
const OP_FMUL: u16 = 133;
const OP_UDIV: u16 = 134;
const OP_SDIV: u16 = 135;
const OP_FDIV: u16 = 136;
const OP_SNEGATE: u16 = 126;
const OP_NOT: u16 = 200;
const OP_LOGICAL_NOT: u16 = 168;
const OP_CONVERT_F_TO_U: u16 = 109;
const OP_CONVERT_F_TO_S: u16 = 110;
const OP_CONVERT_S_TO_F: u16 = 111;
const OP_CONVERT_U_TO_F: u16 = 112;
const OP_IEQUAL: u16 = 170;
const OP_INOT_EQUAL: u16 = 171;
const OP_UGREATER_THAN: u16 = 172;
const OP_SGREATER_THAN: u16 = 173;
const OP_FORD_EQUAL: u16 = 180;
const OP_FORD_LESS_THAN: u16 = 184;
const OP_SELECT: u16 = 169;
const OP_NAME: u16 = 5;
const OP_MEMBER_NAME: u16 = 6;
const OP_DECORATE: u16 = 71;
const OP_MEMBER_DECORATE: u16 = 72;
const OP_CONTROL_BARRIER: u16 = 224;
const OP_MEMORY_BARRIER: u16 = 225;
const OP_ATOMIC_IADD: u16 = 234;
const OP_ATOMIC_ISUB: u16 = 237;
const OP_COPY_OBJECT: u16 = 123;
const OP_PHI: u16 = 245;
const OP_LOOP_MERGE: u16 = 246;
const OP_SELECTION_MERGE: u16 = 247;
const OP_LABEL: u16 = 248;
const OP_BRANCH: u16 = 249;
const OP_BRANCH_CONDITIONAL: u16 = 250;
const OP_RETURN: u16 = 253;
const OP_RETURN_VALUE: u16 = 254;

// Execution model
const EXEC_MODEL_VERTEX: u32 = 0;
const EXEC_MODEL_FRAGMENT: u32 = 4;
const EXEC_MODEL_GLCOMPUTE: u32 = 5;

// Execution mode
const EXEC_MODE_LOCAL_SIZE: u32 = 16;

// Storage class (SPIR-V unified spec)
const STORAGE_UNIFORM_CONSTANT: u32 = 0;
const STORAGE_INPUT: u32 = 1;
const STORAGE_UNIFORM: u32 = 2;
const STORAGE_OUTPUT: u32 = 3;
const STORAGE_WORKGROUP: u32 = 4;
const STORAGE_FUNCTION: u32 = 7;
const STORAGE_PRIVATE: u32 = 6;
const STORAGE_STORAGE_BUFFER: u32 = 12;

// Decoration
const DECORATION_BINDING: u32 = 34;
const DECORATION_DESCRIPTOR_SET: u32 = 32;
const DECORATION_BUILT_IN: u32 = 11;
const DECORATION_LOCATION: u32 = 30;
const DECORATION_OFFSET: u32 = 35;

// Built-in (SPIR-V)
const BUILTIN_NUM_WORK_GROUPS: u32 = 24;
const BUILTIN_WORKGROUP_SIZE: u32 = 25;
const BUILTIN_WORKGROUP_ID: u32 = 26;
const BUILTIN_LOCAL_INVOCATION_ID: u32 = 27;
const BUILTIN_GLOBAL_INVOCATION_ID: u32 = 28;
const BUILTIN_LOCAL_INVOCATION_INDEX: u32 = 29;

#[inline]
fn err(offset: usize, msg: impl Into<String>) -> ParseError {
    ParseError::Syntax {
        offset: offset as u32,
        message: msg.into(),
    }
}

fn read_spirv_string(words: &[u32], word_idx: usize) -> Result<(String, usize), ParseError> {
    let mut bytes = Vec::new();
    let mut i = word_idx;
    while i < words.len() {
        let w = words[i];
        i += 1;
        for b in 0..4 {
            let byte = ((w >> (b * 8)) & 0xff) as u8;
            if byte == 0 {
                return String::from_utf8(bytes)
                    .map(|s| (s, i))
                    .map_err(|e| err(word_idx * 4, format!("invalid UTF-8 in SPIR-V string: {e}")));
            }
            bytes.push(byte);
        }
    }
    Err(err(
        word_idx * 4,
        String::from("unterminated SPIR-V string literal"),
    ))
}

#[derive(Debug, Clone)]
#[allow(unused)]
enum RawType {
    Void,
    Bool,
    Int { width: u32, signed: bool },
    Float { width: u32 },
    Vector { component: u32, count: u32 },
    Matrix { column_type: u32, columns: u32, rows: u32 },
    Array { element: u32, length_id: Option<u32> },
    RuntimeArray { element: u32 },
    Struct { members: Vec<u32> },
    Pointer { storage: u32, pointee: u32 },
    Function { ret: u32, params: Vec<u32> },
    Image {
        sampled_type: u32,
        dim: u32,
        depth: u32,
        arrayed: bool,
        ms: bool,
        sampled: u32,
        format: u32,
    },
    SampledImage { image_type: u32 },
    Sampler,
}

#[derive(Debug, Clone)]
enum ConstVal {
    Bool(bool),
    Scalar(Literal),
    Composite { constituents: Vec<u32> },
}

#[derive(Debug, Default, Clone)]
struct IdDecorations {
    pub binding: Option<u32>,
    pub descriptor_set: Option<u32>,
    pub built_in: Option<u32>,
    pub location: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug)]
struct EntryPointRaw {
    pub model: u32,
    pub fn_id: u32,
    pub name: String,
}

/// Collected module-level state between passes.
struct Pass1 {
    pub capabilities: Vec<u32>,
    pub ext_glsl450: Option<u32>,
    pub memory_model: Option<(u32, u32)>,
    pub entry_points: Vec<EntryPointRaw>,
    /// function id -> local size [x,y,z] if set
    pub local_size: FxHashMap<u32, [u32; 3]>,
    pub names: FxHashMap<u32, String>,
    pub member_names: FxHashMap<(u32, u32), String>,
    pub decorations: FxHashMap<u32, IdDecorations>,
    pub member_decorations: FxHashMap<(u32, u32), IdDecorations>,
    pub raw_types: FxHashMap<u32, RawType>,
    pub constants: FxHashMap<u32, (u32, ConstVal)>,
    /// OpVariable at module scope: result id -> (result_type_id, storage, maybe initializer)
    pub global_vars: Vec<(u32, u32, u32, Option<u32>)>,
    /// function result id -> (result_type, control, fn_type_id, body instruction slice in `words`)
    pub functions: Vec<(u32, u32, u32, u32, Range)>,
    pub words: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
struct Range {
    pub start: usize,
    pub end: usize,
}

fn insn_words(words: &[u32], pos: usize) -> Result<(u16, usize, usize), ParseError> {
    if pos >= words.len() {
        return Err(err(pos * 4, "unexpected end of SPIR-V module"));
    }
    let w0 = words[pos];
    let wc = (w0 >> 16) as usize;
    let op = (w0 & 0xffff) as u16;
    if wc == 0 {
        return Err(err(pos * 4, "SPIR-V instruction word count is zero"));
    }
    if pos + wc > words.len() {
        return Err(err(
            pos * 4,
            format!("SPIR-V instruction extends past module end (op={op}, words={wc})"),
        ));
    }
    Ok((op, wc, pos))
}

fn collect_pass1(words: &[u32]) -> Result<Pass1, ParseError> {
    let mut p = Pass1 {
        capabilities: Vec::new(),
        ext_glsl450: None,
        memory_model: None,
        entry_points: Vec::new(),
        local_size: FxHashMap::default(),
        names: FxHashMap::default(),
        member_names: FxHashMap::default(),
        decorations: FxHashMap::default(),
        member_decorations: FxHashMap::default(),
        raw_types: FxHashMap::default(),
        constants: FxHashMap::default(),
        global_vars: Vec::new(),
        functions: Vec::new(),
        words: words.to_vec(),
    };

    let mut i = 5usize;
    while i < words.len() {
        let (op, wc, base) = insn_words(words, i)?;
        let operands = &words[base + 1..base + wc];

        match op {
            OP_CAPABILITY => {
                if let Some(&cap) = operands.first() {
                    p.capabilities.push(cap);
                }
            }
            OP_EXT_INST_IMPORT => {
                if operands.len() >= 2 {
                    let id = operands[0];
                    let (s, _) = read_spirv_string(words, base + 2)?;
                    if s == "GLSL.std.450" {
                        p.ext_glsl450 = Some(id);
                    }
                }
            }
            OP_MEMORY_MODEL => {
                if operands.len() >= 2 {
                    p.memory_model = Some((operands[0], operands[1]));
                }
            }
            OP_ENTRY_POINT => {
                if operands.len() >= 3 {
                    let model = operands[0];
                    let fn_id = operands[1];
                    let (name, _after) = read_spirv_string(words, base + 3)?;
                    p.entry_points.push(EntryPointRaw { model, fn_id, name });
                }
            }
            OP_EXECUTION_MODE => {
                if operands.len() >= 2 {
                    let fn_id = operands[0];
                    let mode = operands[1];
                    if mode == EXEC_MODE_LOCAL_SIZE && operands.len() >= 5 {
                        p.local_size.insert(fn_id, [operands[2], operands[3], operands[4]]);
                    }
                }
            }
            OP_NAME => {
                if operands.len() >= 2 {
                    let id = operands[0];
                    let (name, _) = read_spirv_string(words, base + 2)?;
                    p.names.insert(id, name);
                }
            }
            OP_MEMBER_NAME => {
                if operands.len() >= 3 {
                    let id = operands[0];
                    let m = operands[1];
                    let (name, _) = read_spirv_string(words, base + 3)?;
                    p.member_names.insert((id, m), name);
                }
            }
            OP_DECORATE => {
                if operands.len() >= 2 {
                    let id = operands[0];
                    let dec = operands[1];
                    let d = p.decorations.entry(id).or_default();
                    match dec {
                        DECORATION_BINDING if operands.len() >= 3 => d.binding = Some(operands[2]),
                        DECORATION_DESCRIPTOR_SET if operands.len() >= 3 => {
                            d.descriptor_set = Some(operands[2])
                        }
                        DECORATION_BUILT_IN if operands.len() >= 3 => d.built_in = Some(operands[2]),
                        DECORATION_LOCATION if operands.len() >= 3 => d.location = Some(operands[2]),
                        DECORATION_OFFSET if operands.len() >= 3 => d.offset = Some(operands[2]),
                        _ => {}
                    }
                }
            }
            OP_MEMBER_DECORATE => {
                if operands.len() >= 3 {
                    let id = operands[0];
                    let m = operands[1];
                    let dec = operands[2];
                    let d = p.member_decorations.entry((id, m)).or_default();
                    match dec {
                        DECORATION_BINDING if operands.len() >= 4 => d.binding = Some(operands[3]),
                        DECORATION_DESCRIPTOR_SET if operands.len() >= 4 => {
                            d.descriptor_set = Some(operands[3])
                        }
                        DECORATION_BUILT_IN if operands.len() >= 4 => d.built_in = Some(operands[3]),
                        DECORATION_LOCATION if operands.len() >= 4 => d.location = Some(operands[3]),
                        DECORATION_OFFSET if operands.len() >= 4 => d.offset = Some(operands[3]),
                        _ => {}
                    }
                }
            }
            OP_TYPE_VOID => {
                if let Some(&rid) = operands.first() {
                    p.raw_types.insert(rid, RawType::Void);
                }
            }
            OP_TYPE_BOOL => {
                if let Some(&rid) = operands.first() {
                    p.raw_types.insert(rid, RawType::Bool);
                }
            }
            OP_TYPE_INT => {
                if operands.len() >= 3 {
                    let rid = operands[0];
                    let width = operands[1];
                    let signed = operands[2] != 0;
                    p.raw_types.insert(rid, RawType::Int { width, signed });
                }
            }
            OP_TYPE_FLOAT => {
                if operands.len() >= 2 {
                    let rid = operands[0];
                    let width = operands[1];
                    p.raw_types.insert(rid, RawType::Float { width });
                }
            }
            OP_TYPE_VECTOR => {
                if operands.len() >= 3 {
                    p.raw_types.insert(
                        operands[0],
                        RawType::Vector {
                            component: operands[1],
                            count: operands[2],
                        },
                    );
                }
            }
            OP_TYPE_MATRIX => {
                if operands.len() >= 3 {
                    let col_ty = operands[1];
                    let cols = operands[2];
                    let rows = match p.raw_types.get(&col_ty) {
                        Some(RawType::Vector { count, .. }) => *count,
                        _ => cols,
                    };
                    p.raw_types.insert(
                        operands[0],
                        RawType::Matrix {
                            column_type: col_ty,
                            columns: cols,
                            rows,
                        },
                    );
                }
            }
            OP_TYPE_IMAGE => {
                if operands.len() >= 9 {
                    p.raw_types.insert(
                        operands[0],
                        RawType::Image {
                            sampled_type: operands[1],
                            dim: operands[2],
                            depth: operands[3],
                            arrayed: operands[4] != 0,
                            ms: operands[5] != 0,
                            sampled: operands[6],
                            format: operands[8],
                        },
                    );
                }
            }
            OP_TYPE_SAMPLER => {
                if let Some(&rid) = operands.first() {
                    p.raw_types.insert(rid, RawType::Sampler);
                }
            }
            OP_TYPE_SAMPLED_IMAGE => {
                if operands.len() >= 2 {
                    p.raw_types.insert(
                        operands[0],
                        RawType::SampledImage {
                            image_type: operands[1],
                        },
                    );
                }
            }
            OP_TYPE_ARRAY => {
                if operands.len() >= 3 {
                    p.raw_types.insert(
                        operands[0],
                        RawType::Array {
                            element: operands[1],
                            length_id: Some(operands[2]),
                        },
                    );
                }
            }
            OP_TYPE_RUNTIME_ARRAY => {
                if operands.len() >= 2 {
                    p.raw_types.insert(
                        operands[0],
                        RawType::RuntimeArray {
                            element: operands[1],
                        },
                    );
                }
            }
            OP_TYPE_STRUCT => {
                if !operands.is_empty() {
                    let rid = operands[0];
                    let members: Vec<u32> = operands[1..].to_vec();
                    p.raw_types.insert(rid, RawType::Struct { members });
                }
            }
            OP_TYPE_POINTER => {
                if operands.len() >= 3 {
                    p.raw_types.insert(
                        operands[0],
                        RawType::Pointer {
                            storage: operands[1],
                            pointee: operands[2],
                        },
                    );
                }
            }
            OP_TYPE_FUNCTION => {
                if !operands.is_empty() {
                    let rid = operands[0];
                    let ret = operands[1];
                    let params: Vec<u32> = operands[2..].to_vec();
                    p.raw_types.insert(rid, RawType::Function { ret, params });
                }
            }
            OP_CONSTANT_TRUE => {
                // OpConstantTrue: Result Type, Result <id>
                if operands.len() >= 2 {
                    let ty = operands[0];
                    let result = operands[1];
                    p.constants
                        .insert(result, (ty, ConstVal::Bool(true)));
                }
            }
            OP_CONSTANT_FALSE => {
                if operands.len() >= 2 {
                    let ty = operands[0];
                    let result = operands[1];
                    p.constants
                        .insert(result, (ty, ConstVal::Bool(false)));
                }
            }
            OP_CONSTANT => {
                // OpConstant: Result Type, Result <id>, literal...
                if operands.len() >= 3 {
                    let ty = operands[0];
                    let result = operands[1];
                    let words_const: Vec<u32> = operands[2..].to_vec();
                    let val = resolve_constant_literal(ty, &words_const, &p)?;
                    p.constants.insert(result, (ty, val));
                }
            }
            OP_CONSTANT_COMPOSITE => {
                if operands.len() >= 2 {
                    let ty = operands[0];
                    let result = operands[1];
                    let parts: Vec<u32> = operands[2..].to_vec();
                    p.constants.insert(
                        result,
                        (
                            ty,
                            ConstVal::Composite {
                                constituents: parts,
                            },
                        ),
                    );
                }
            }
            OP_VARIABLE => {
                if operands.len() >= 3 {
                    let result_type = operands[0];
                    let result = operands[1];
                    let storage = operands[2];
                    let init = operands.get(3).copied();
                    if storage != STORAGE_FUNCTION {
                        p.global_vars.push((result, result_type, storage, init));
                    }
                }
            }
            OP_FUNCTION => {
                if operands.len() >= 4 {
                    let result_type = operands[0];
                    let result = operands[1];
                    let control = operands[2];
                    let fn_ty = operands[3];
                    let body_start = base + wc;
                    let body_end = find_function_end(words, body_start)?;
                    p.functions.push((result, result_type, control, fn_ty, Range {
                        start: body_start,
                        end: body_end,
                    }));
                    i = body_end;
                    continue;
                }
            }
            _ => {}
        }
        i += wc;
    }

    Ok(p)
}

fn find_function_end(words: &[u32], mut pos: usize) -> Result<usize, ParseError> {
    while pos < words.len() {
        let (op, wc, _) = insn_words(words, pos)?;
        if op == OP_FUNCTION_END {
            return Ok(pos + wc);
        }
        pos += wc;
    }
    Err(err(
        words.len() * 4,
        "missing OpFunctionEnd",
    ))
}

fn resolve_constant_literal(
    ty_id: u32,
    words: &[u32],
    p: &Pass1,
) -> Result<ConstVal, ParseError> {
    match p.raw_types.get(&ty_id) {
        Some(RawType::Int { width, signed }) => {
            let v = *words.first().ok_or_else(|| err(0, "constant missing literal words"))?;
            if *width == 32 {
                if *signed {
                    Ok(ConstVal::Scalar(Literal::I32(v as i32)))
                } else {
                    Ok(ConstVal::Scalar(Literal::U32(v)))
                }
            } else {
                Err(ParseError::Unsupported(format!(
                    "integer width {width} not supported"
                )))
            }
        }
        Some(RawType::Float { width }) => {
            let v = *words.first().ok_or_else(|| err(0, "constant missing literal words"))?;
            if *width == 32 {
                Ok(ConstVal::Scalar(Literal::F32(f32::from_bits(v))))
            } else if *width == 64 {
                let lo = v;
                let hi = words.get(1).copied().unwrap_or(0);
                let bits = (u64::from(hi) << 32) | u64::from(lo);
                Ok(ConstVal::Scalar(Literal::F64(f64::from_bits(bits))))
            } else {
                Err(ParseError::Unsupported(format!(
                    "float width {width} not supported"
                )))
            }
        }
        Some(RawType::Bool) => Err(err(0, "bool constant should use OpConstantTrue/False")),
        _ => Err(ParseError::Unsupported(format!(
            "constant of type id {ty_id} not supported"
        ))),
    }
}

fn spirv_dim(dim: u32) -> ImageDimension {
    match dim {
        0 => ImageDimension::D1,
        1 => ImageDimension::D2,
        2 => ImageDimension::D3,
        3 => ImageDimension::Cube,
        _ => ImageDimension::D2,
    }
}

fn scalar_from_sampled_ty(
    ty_id: u32,
    raw: &FxHashMap<u32, RawType>,
) -> Result<Scalar, ParseError> {
    match raw.get(&ty_id) {
        Some(RawType::Int { width, signed }) if *width == 32 => {
            if *signed {
                Ok(Scalar::I32)
            } else {
                Ok(Scalar::U32)
            }
        }
        Some(RawType::Float { width }) if *width == 32 => Ok(Scalar::F32),
        Some(RawType::Bool) => Ok(Scalar::BOOL),
        _ => Err(ParseError::Unsupported(format!(
            "sampled type id {ty_id}"
        ))),
    }
}

fn vector_size(n: u32) -> Result<VectorSize, ParseError> {
    match n {
        2 => Ok(VectorSize::Bi),
        3 => Ok(VectorSize::Tri),
        4 => Ok(VectorSize::Quad),
        _ => Err(ParseError::Unsupported(format!(
            "vector size {n} not supported"
        ))),
    }
}

fn array_length_from_id(
    length_id: u32,
    p1: &Pass1,
) -> Result<ArraySize, ParseError> {
    if let Some((_, cv)) = p1.constants.get(&length_id) {
        match cv {
            ConstVal::Scalar(Literal::U32(n)) => Ok(ArraySize::Constant(*n)),
            ConstVal::Scalar(Literal::I32(i)) if *i >= 0 => Ok(ArraySize::Constant(*i as u32)),
            _ => Ok(ArraySize::Dynamic),
        }
    } else {
        Ok(ArraySize::Dynamic)
    }
}

fn storage_class_ast(sc: u32) -> Result<AddressSpace, ParseError> {
    match sc {
        STORAGE_UNIFORM | STORAGE_UNIFORM_CONSTANT => Ok(AddressSpace::Uniform),
        STORAGE_STORAGE_BUFFER => Ok(AddressSpace::Storage {
            access: StorageAccess::LoadStore,
        }),
        STORAGE_WORKGROUP => Ok(AddressSpace::WorkGroup),
        STORAGE_PRIVATE => Ok(AddressSpace::Private),
        STORAGE_FUNCTION => Ok(AddressSpace::Function),
        STORAGE_INPUT | STORAGE_OUTPUT => Ok(AddressSpace::Handle),
        _ => Err(ParseError::Unsupported(format!(
            "SPIR-V storage class {sc}"
        ))),
    }
}

fn spirv_builtin_to_ast(b: u32) -> Result<BuiltIn, ParseError> {
    match b {
        BUILTIN_NUM_WORK_GROUPS => Ok(BuiltIn::NumWorkGroups),
        BUILTIN_WORKGROUP_SIZE => Ok(BuiltIn::WorkGroupSize),
        BUILTIN_WORKGROUP_ID => Ok(BuiltIn::WorkGroupId),
        BUILTIN_LOCAL_INVOCATION_ID => Ok(BuiltIn::LocalInvocationId),
        BUILTIN_GLOBAL_INVOCATION_ID => Ok(BuiltIn::GlobalInvocationId),
        BUILTIN_LOCAL_INVOCATION_INDEX => Ok(BuiltIn::LocalInvocationIndex),
        _ => Err(ParseError::Unsupported(format!(
            "SPIR-V BuiltIn decoration {b}"
        ))),
    }
}

fn binding_from_decor(d: &IdDecorations) -> Option<Binding> {
    if let Some(b) = d.built_in {
        spirv_builtin_to_ast(b).ok().map(Binding::BuiltIn)
    } else if let Some(loc) = d.location {
        Some(Binding::Location {
            location: loc,
            interpolation: None,
            sampling: None,
        })
    } else {
        None
    }
}

fn build_type_handle(
    id: u32,
    raw: &FxHashMap<u32, RawType>,
    p1: &Pass1,
    module: &mut Module,
    cache: &mut FxHashMap<u32, Handle<Type>>,
) -> Result<Handle<Type>, ParseError> {
    if let Some(&h) = cache.get(&id) {
        return Ok(h);
    }
    let rt = raw
        .get(&id)
        .ok_or_else(|| ParseError::Unsupported(format!("unknown type id {id}")))?
        .clone();

    let h = match rt {
        RawType::Void => {
            return Err(ParseError::Unsupported("void type in value position".into()));
        }
        RawType::Bool => module.types.append(Type::Bool),
        RawType::Int { width, signed } => {
            if width != 32 {
                return Err(ParseError::Unsupported(format!("OpTypeInt width {width}")));
            }
            module.types.append(Type::Scalar(if signed {
                Scalar::I32
            } else {
                Scalar::U32
            }))
        }
        RawType::Float { width } => {
            if width == 32 {
                module.types.append(Type::Scalar(Scalar::F32))
            } else if width == 64 {
                module.types.append(Type::Scalar(Scalar::F64))
            } else {
                return Err(ParseError::Unsupported(format!("OpTypeFloat width {width}")));
            }
        }
        RawType::Vector { component, count } => {
            let sc = scalar_from_sampled_ty(component, raw)?;
            let vs = vector_size(count)?;
            module.types.append(Type::Vector {
                scalar: sc,
                size: vs,
            })
        }
        RawType::Matrix {
            column_type,
            columns,
            rows,
        } => {
            let col_h = build_type_handle(column_type, raw, p1, module, cache)?;
            match module.types.get(col_h) {
                Type::Vector { scalar, .. } => {
                    let cs = vector_size(columns)?;
                    let rs = vector_size(rows)?;
                    module.types.append(Type::Matrix {
                        scalar: *scalar,
                        columns: cs,
                        rows: rs,
                    })
                }
                _ => {
                    return Err(ParseError::Unsupported(
                        "matrix column type must be vector".into(),
                    ));
                }
            }
        }
        RawType::Array { element, length_id } => {
            let base = build_type_handle(element, raw, p1, module, cache)?;
            let size = if let Some(lid) = length_id {
                array_length_from_id(lid, p1)?
            } else {
                ArraySize::Dynamic
            };
            module.types.append(Type::Array { base, size })
        }
        RawType::RuntimeArray { element } => {
            let base = build_type_handle(element, raw, p1, module, cache)?;
            module.types.append(Type::Array {
                base,
                size: ArraySize::Dynamic,
            })
        }
        RawType::Struct { members } => {
            let name = p1.names.get(&id).cloned();
            let mut sm = Vec::new();
            for (i, &mty) in members.iter().enumerate() {
                let ty = build_type_handle(mty, raw, p1, module, cache)?;
                let mname = p1.member_names.get(&(id, i as u32)).cloned();
                let dec = p1.member_decorations.get(&(id, i as u32)).cloned();
                let offset = dec.as_ref().and_then(|d| d.offset);
                let binding = dec.as_ref().and_then(binding_from_decor);
                sm.push(StructMember {
                    name: mname,
                    ty,
                    offset,
                    binding,
                });
            }
            module.types.append(Type::Struct { name, members: sm })
        }
        RawType::Pointer { storage, pointee } => {
            let base = build_type_handle(pointee, raw, p1, module, cache)?;
            let space = storage_class_ast(storage)?;
            module.types.append(Type::Pointer { base, space })
        }
        RawType::Function { .. } => {
            return Err(ParseError::Unsupported("OpTypeFunction as value type".into()));
        }
        RawType::Image {
            sampled_type: _,
            dim,
            depth: _,
            arrayed,
            ms,
            sampled,
            format: _,
        } => {
            let sample_ty = if sampled == 2 {
                TextureSampleType::Depth
            } else {
                TextureSampleType::Float { filterable: true }
            };
            module.types.append(Type::Texture {
                dim: spirv_dim(dim),
                arrayed,
                multisampled: ms,
                sample_type: sample_ty,
            })
        }
        RawType::SampledImage { image_type } => {
            build_type_handle(image_type, raw, p1, module, cache)?
        }
        RawType::Sampler => module.types.append(Type::Sampler { comparison: false }),
    };

    cache.insert(id, h);
    Ok(h)
}

/// SPIR-V → AST.
pub fn parse(words: &[u32]) -> Result<Module, ParseError> {
    if words.len() < 5 {
        return Err(err(0, "SPIR-V module too short"));
    }
    if words[0] != SPIRV_MAGIC {
        return Err(err(0, format!("invalid SPIR-V magic: {:#x}", words[0])));
    }
    let _version = words[1];
    let _generator = words[2];
    let _bound = words[3];
    let _schema = words[4];

    let p1 = collect_pass1(words)?;
    let mut module = Module::new();
    let mut type_cache: FxHashMap<u32, Handle<Type>> = FxHashMap::default();

    for id in p1.raw_types.keys().copied().collect::<Vec<_>>() {
        if type_cache.contains_key(&id) {
            continue;
        }
        let _ = build_type_handle(id, &p1.raw_types, &p1, &mut module, &mut type_cache)?;
    }

    let mut spirv_global_to_index: FxHashMap<u32, u32> = FxHashMap::default();

    for (idx, &(var_id, ptr_ty_id, storage_class, _init)) in p1.global_vars.iter().enumerate() {
        let pointee_ty = match p1.raw_types.get(&ptr_ty_id) {
            Some(RawType::Pointer { pointee, .. }) => *pointee,
            _ => {
                return Err(ParseError::Unsupported(
                    "global OpVariable must have pointer type".into(),
                ));
            }
        };
        let ty = build_type_handle(pointee_ty, &p1.raw_types, &p1, &mut module, &mut type_cache)?;
        let space = storage_class_ast(storage_class)?;
        let name = p1.names.get(&var_id).cloned();
        let binding = p1.decorations.get(&var_id).and_then(|d| {
            if let (Some(g), Some(b)) = (d.descriptor_set, d.binding) {
                Some(ResourceBinding {
                    group: g,
                    binding: b,
                })
            } else {
                None
            }
        });
        module.global_variables.push(GlobalVariable {
            name,
            space,
            binding,
            ty,
        });
        spirv_global_to_index.insert(var_id, idx as u32);
    }

    if p1.entry_points.is_empty() {
        return Err(ParseError::Unsupported(
            "no OpEntryPoint in SPIR-V module".into(),
        ));
    }

    for ep in &p1.entry_points {
        let fn_id = ep.fn_id;
        let Some((func_result_id, func_ret_ty, _control, fn_ty_id, body)) = p1
            .functions
            .iter()
            .find(|(rid, _, _, _, _)| *rid == fn_id)
            .copied()
        else {
            return Err(ParseError::Unsupported(format!(
                "entry point function id {fn_id} not found"
            )));
        };
        let _ = func_ret_ty;
        let params = match p1.raw_types.get(&fn_ty_id) {
            Some(RawType::Function { params, .. }) => params.clone(),
            _ => {
                return Err(ParseError::Unsupported(
                    "entry point must use OpTypeFunction".into(),
                ));
            }
        };

        let wg = p1
            .local_size
            .get(&fn_id)
            .copied()
            .unwrap_or([1, 1, 1]);

        let stage = match ep.model {
            EXEC_MODEL_GLCOMPUTE => ShaderStage::Compute,
            EXEC_MODEL_VERTEX => ShaderStage::Vertex,
            EXEC_MODEL_FRAGMENT => ShaderStage::Fragment,
            _ => {
                return Err(ParseError::Unsupported(format!(
                    "execution model {}",
                    ep.model
                )));
            }
        };

        let mut func = Function::new();
        func.name = p1.names.get(&fn_id).cloned();

        let body_slice = &p1.words[body.start..body.end];
        let mut fn_ctx = FnCtx {
            p1: &p1,
            module: &mut module,
            type_cache: &mut type_cache,
            spirv_global_to_index: &spirv_global_to_index,
            expr_ids: FxHashMap::default(),
        };

        let mut i = 0usize;
        let mut param_idx: u32 = 0;
        let mut param_spirv_ids = Vec::new();
        while i < body_slice.len() {
            let (op, wc, base) = insn_words(body_slice, i)?;
            let operands = &body_slice[base + 1..base + wc];
            if op == OP_FUNCTION_PARAMETER {
                if operands.len() >= 2 {
                    let pty = operands[0];
                    let pid = operands[1];
                    let expected = *params.get(param_idx as usize).ok_or_else(|| {
                        ParseError::Unsupported("too many OpFunctionParameter".into())
                    })?;
                    if pty != expected {
                        return Err(ParseError::Unsupported(
                            "OpFunctionParameter type mismatch with OpTypeFunction".into(),
                        ));
                    }
                    let ty_h = build_type_handle(pty, &p1.raw_types, &p1, fn_ctx.module, fn_ctx.type_cache)?;
                    let binding = p1.decorations.get(&pid).and_then(binding_from_decor);
                    func.arguments.push(FunctionArgument {
                        name: p1.names.get(&pid).cloned(),
                        ty: ty_h,
                        binding,
                    });
                    param_spirv_ids.push(pid);
                    param_idx += 1;
                }
            } else {
                break;
            }
            i += wc;
        }

        if param_idx as usize != params.len() {
            return Err(ParseError::Unsupported(
                "OpFunctionParameter count mismatch".into(),
            ));
        }

        translate_function_body(
            &mut func,
            &mut fn_ctx,
            body_slice,
            i,
            func_result_id,
            &param_spirv_ids,
        )?;

        module.entry_points.push(EntryPoint {
            name: ep.name.clone(),
            stage,
            workgroup_size: wg,
            function: func,
        });
    }

    Ok(module)
}

struct FnCtx<'a> {
    p1: &'a Pass1,
    module: &'a mut Module,
    type_cache: &'a mut FxHashMap<u32, Handle<Type>>,
    spirv_global_to_index: &'a FxHashMap<u32, u32>,
    expr_ids: FxHashMap<u32, Handle<Expression>>,
}

impl FnCtx<'_> {
    fn ty_handle(&mut self, id: u32) -> Result<Handle<Type>, ParseError> {
        build_type_handle(id, &self.p1.raw_types, self.p1, self.module, self.type_cache)
    }

    fn get_or_const_expr(&mut self, func: &mut Function, spirv_id: u32) -> Result<Handle<Expression>, ParseError> {
        if let Some(&h) = self.expr_ids.get(&spirv_id) {
            return Ok(h);
        }
        if let Some(&gix) = self.spirv_global_to_index.get(&spirv_id) {
            let h = func
                .expressions
                .append(Expression::GlobalVariable(gix));
            self.expr_ids.insert(spirv_id, h);
            return Ok(h);
        }
        if let Some((ty_id, cv)) = self.p1.constants.get(&spirv_id) {
            let h = match cv {
                ConstVal::Bool(b) => {
                    func.expressions.append(Expression::Literal(Literal::Bool(*b)))
                }
                ConstVal::Scalar(lit) => func.expressions.append(Expression::Literal(*lit)),
                ConstVal::Composite { constituents } => {
                    let ty = self.ty_handle(*ty_id)?;
                    let mut comps = Vec::new();
                    for &c in constituents {
                        comps.push(self.get_or_const_expr(func, c)?);
                    }
                    func.expressions.append(Expression::Compose {
                        ty,
                        components: comps,
                    })
                }
            };
            self.expr_ids.insert(spirv_id, h);
            return Ok(h);
        }
        Err(ParseError::Unsupported(format!(
            "no expression for id {spirv_id}"
        )))
    }
}

fn translate_function_body(
    func: &mut Function,
    ctx: &mut FnCtx<'_>,
    body: &[u32],
    start_idx: usize,
    _func_result_id: u32,
    param_spirv_ids: &[u32],
) -> Result<(), ParseError> {
    for (i, &pid) in param_spirv_ids.iter().enumerate() {
        let h = func
            .expressions
            .append(Expression::FunctionArgument(i as u32));
        ctx.expr_ids.insert(pid, h);
    }

    let insns = decode_insns(body, start_idx)?;
    let blocks = split_basic_blocks(&insns)?;
    let entry = blocks
        .first()
        .ok_or_else(|| ParseError::Unsupported("function has no basic blocks".into()))?
        .label;
    let ordered = linearize_cfg(entry, &blocks)?;
    let mut stmts = Vec::new();
    for lid in ordered {
        let bb = blocks.iter().find(|b| b.label == lid)
            .ok_or_else(|| err(0, format!("missing basic block with label {lid}")))?;
        for insn in &bb.instructions {
            translate_insn(func, ctx, insn, &mut stmts)?;
        }
    }
    func.body = stmts;
    Ok(())
}

#[derive(Debug)]
struct BasicBlock {
    label: u32,
    instructions: Vec<DecodedInsn>,
}

#[derive(Debug)]
struct DecodedInsn {
    opcode: u16,
    operands: Vec<u32>,
}

fn decode_insns(body: &[u32], mut pos: usize) -> Result<Vec<DecodedInsn>, ParseError> {
    let mut out = Vec::new();
    while pos < body.len() {
        let (op, wc, base) = insn_words(body, pos)?;
        let operands = body[base + 1..base + wc].to_vec();
        out.push(DecodedInsn {
            opcode: op,
            operands,
        });
        pos += wc;
    }
    Ok(out)
}

fn split_basic_blocks(insns: &[DecodedInsn]) -> Result<Vec<BasicBlock>, ParseError> {
    let mut blocks = Vec::new();
    let mut cur: Option<BasicBlock> = None;
    for insn in insns {
        if insn.opcode == OP_LABEL {
            if let Some(b) = cur.take() {
                blocks.push(b);
            }
            let label = *insn
                .operands
                .first()
                .ok_or_else(|| ParseError::Unsupported("OpLabel without id".into()))?;
            cur = Some(BasicBlock {
                label,
                instructions: Vec::new(),
            });
        } else if let Some(ref mut b) = cur {
            b.instructions.push(DecodedInsn {
                opcode: insn.opcode,
                operands: insn.operands.clone(),
            });
        } else {
            return Err(ParseError::Unsupported(
                "instruction before OpLabel".into(),
            ));
        }
    }
    if let Some(b) = cur {
        blocks.push(b);
    }
    Ok(blocks)
}

fn linearize_cfg(
    entry: u32,
    blocks: &[BasicBlock],
) -> Result<Vec<u32>, ParseError> {
    let map: FxHashMap<u32, &BasicBlock> = blocks.iter().map(|b| (b.label, b)).collect();
    let mut order = Vec::new();
    let mut cur = entry;
    loop {
        order.push(cur);
        let bb = map
            .get(&cur)
            .ok_or_else(|| ParseError::Unsupported("unknown block label".into()))?;
        let term = bb
            .instructions
            .last()
            .ok_or_else(|| ParseError::Unsupported("empty basic block".into()))?;
        match term.opcode {
            OP_RETURN | OP_RETURN_VALUE => return Ok(order),
            OP_BRANCH => {
                cur = *term
                    .operands
                    .first()
                    .ok_or_else(|| ParseError::Unsupported("OpBranch target missing".into()))?;
            }
            OP_BRANCH_CONDITIONAL | OP_LOOP_MERGE | OP_SELECTION_MERGE | OP_PHI => {
                return Err(ParseError::Unsupported(
                    "non-linear control flow not supported in SPIR-V reader yet".into(),
                ));
            }
            _ => {
                return Err(ParseError::Unsupported(format!(
                    "unsupported terminator opcode {}",
                    term.opcode
                )));
            }
        }
    }
}

fn translate_insn(
    func: &mut Function,
    ctx: &mut FnCtx<'_>,
    insn: &DecodedInsn,
    stmts: &mut Vec<Statement>,
) -> Result<(), ParseError> {
    let op = insn.opcode;
    let o = &insn.operands;

    match op {
        OP_LABEL => Ok(()),
        OP_SELECTION_MERGE | OP_LOOP_MERGE => Ok(()),
        OP_RETURN => {
            stmts.push(Statement::Return { value: None });
            Ok(())
        }
        OP_RETURN_VALUE => {
            if let Some(&v) = o.first() {
                let h = ctx.get_or_const_expr(func, v)?;
                stmts.push(Statement::Return { value: Some(h) });
            }
            Ok(())
        }
        OP_STORE => {
            if o.len() >= 2 {
                let ptr = o[0];
                let val = o[1];
                let pe = ctx.get_or_const_expr(func, ptr)?;
                let ve = ctx.get_or_const_expr(func, val)?;
                stmts.push(Statement::Store {
                    pointer: pe,
                    value: ve,
                });
            }
            Ok(())
        }
        OP_CONTROL_BARRIER => {
            stmts.push(Statement::ControlBarrier(Barrier::ALL));
            Ok(())
        }
        OP_MEMORY_BARRIER => {
            stmts.push(Statement::MemoryBarrier(Barrier::STORAGE));
            Ok(())
        }
        OP_VARIABLE => {
            if o.len() >= 2 {
                let result_ty = o[0];
                let result_id = o[1];
                let _storage = o.get(2).copied().unwrap_or(STORAGE_FUNCTION);
                let pointee = match ctx.p1.raw_types.get(&result_ty) {
                    Some(RawType::Pointer { pointee, .. }) => *pointee,
                    _ => {
                        return Err(ParseError::Unsupported(
                            "OpVariable expects pointer result type".into(),
                        ));
                    }
                };
                let ty = ctx.ty_handle(pointee)?;
                let init = o.get(3).map(|&i| ctx.get_or_const_expr(func, i)).transpose()?;
                let idx = func.local_variables.len() as u32;
                func.local_variables.push(LocalVariable {
                    name: ctx.p1.names.get(&result_id).cloned(),
                    ty,
                    init,
                });
                let ptr_h = func.expressions.append(Expression::LocalVariable(idx));
                ctx.expr_ids.insert(result_id, ptr_h);
            }
            Ok(())
        }
        OP_LOAD => {
            if o.len() >= 3 {
                let result_ty = o[0];
                let result_id = o[1];
                let ptr = o[2];
                let _ = result_ty;
                let pe = ctx.get_or_const_expr(func, ptr)?;
                let load_h = func.expressions.append(Expression::Load { pointer: pe });
                ctx.expr_ids.insert(result_id, load_h);
            }
            Ok(())
        }
        OP_ACCESS_CHAIN | OP_IN_BOUNDS_ACCESS_CHAIN => {
            if o.len() >= 3 {
                let result_ty = o[0];
                let result_id = o[1];
                let base = o[2];
                let indices = &o[3..];
                let _ = result_ty;
                let mut expr = ctx.get_or_const_expr(func, base)?;
                for &idx_id in indices {
                    let idx_e = ctx.get_or_const_expr(func, idx_id)?;
                    match &func.expressions[idx_e] {
                        Expression::Literal(Literal::U32(c)) => {
                            expr = func.expressions.append(Expression::AccessIndex {
                                base: expr,
                                index: *c,
                            });
                        }
                        Expression::Literal(Literal::I32(i)) if *i >= 0 => {
                            expr = func.expressions.append(Expression::AccessIndex {
                                base: expr,
                                index: *i as u32,
                            });
                        }
                        _ => {
                            expr = func.expressions.append(Expression::Access {
                                base: expr,
                                index: idx_e,
                            });
                        }
                    }
                }
                ctx.expr_ids.insert(result_id, expr);
            }
            Ok(())
        }
        OP_COPY_OBJECT => {
            if o.len() >= 2 {
                let result_ty = o[0];
                let result_id = o[1];
                let src = o[2];
                let _ = result_ty;
                let h = ctx.get_or_const_expr(func, src)?;
                ctx.expr_ids.insert(result_id, h);
            }
            Ok(())
        }
        _ => {
            let r = try_emit_expr(func, ctx, insn, stmts)?;
            if let Some((rid, h)) = r {
                ctx.expr_ids.insert(rid, h);
            }
            Ok(())
        }
    }
}

fn try_emit_expr(
    func: &mut Function,
    ctx: &mut FnCtx<'_>,
    insn: &DecodedInsn,
    stmts: &mut Vec<Statement>,
) -> Result<Option<(u32, Handle<Expression>)>, ParseError> {
    let op = insn.opcode;
    let o = &insn.operands;
    if o.len() < 2 {
        return Ok(None);
    }
    let result_ty = o[0];
    let result_id = o[1];
    let _ = result_ty;

    match op {
        OP_IADD | OP_FADD => binary(func, ctx, o, BinaryOp::Add),
        OP_ISUB | OP_FSUB => binary(func, ctx, o, BinaryOp::Subtract),
        OP_IMUL | OP_FMUL => binary(func, ctx, o, BinaryOp::Multiply),
        OP_UDIV | OP_SDIV | OP_FDIV => binary(func, ctx, o, BinaryOp::Divide),
        OP_IEQUAL => binary(func, ctx, o, BinaryOp::Equal),
        OP_INOT_EQUAL => binary(func, ctx, o, BinaryOp::NotEqual),
        OP_UGREATER_THAN | OP_SGREATER_THAN => binary(func, ctx, o, BinaryOp::Greater),
        OP_FORD_EQUAL => binary(func, ctx, o, BinaryOp::Equal),
        OP_FORD_LESS_THAN => binary(func, ctx, o, BinaryOp::Less),
        OP_SELECT => {
            if o.len() >= 5 {
                let cond = ctx.get_or_const_expr(func, o[2])?;
                let t = ctx.get_or_const_expr(func, o[3])?;
                let f = ctx.get_or_const_expr(func, o[4])?;
                let h = func.expressions.append(Expression::Select {
                    condition: cond,
                    accept: t,
                    reject: f,
                });
                return Ok(Some((result_id, h)));
            }
            Ok(None)
        }
        OP_FNEGATE | OP_SNEGATE => {
            if o.len() >= 3 {
                let a = ctx.get_or_const_expr(func, o[2])?;
                let h = func.expressions.append(Expression::Unary {
                    op: UnaryOp::Negate,
                    expr: a,
                });
                return Ok(Some((result_id, h)));
            }
            Ok(None)
        }
        OP_NOT | OP_LOGICAL_NOT => {
            if o.len() >= 3 {
                let a = ctx.get_or_const_expr(func, o[2])?;
                let h = func.expressions.append(Expression::Unary {
                    op: UnaryOp::LogicalNot,
                    expr: a,
                });
                return Ok(Some((result_id, h)));
            }
            Ok(None)
        }
        OP_CONVERT_F_TO_U => cast(func, ctx, o, ScalarKind::Uint),
        OP_CONVERT_F_TO_S => cast(func, ctx, o, ScalarKind::Sint),
        OP_CONVERT_S_TO_F | OP_CONVERT_U_TO_F => cast(func, ctx, o, ScalarKind::Float),
        OP_BITCAST => {
            if o.len() >= 4 {
                let src = ctx.get_or_const_expr(func, o[3])?;
                let h = func.expressions.append(Expression::As {
                    expr: src,
                    kind: ScalarKind::Float,
                    convert: Some(4),
                });
                return Ok(Some((result_id, h)));
            }
            Ok(None)
        }
        OP_COMPOSITE_EXTRACT => {
            if o.len() >= 4 {
                let composite = ctx.get_or_const_expr(func, o[2])?;
                let index = o[3];
                let h = func.expressions.append(Expression::AccessIndex {
                    base: composite,
                    index,
                });
                return Ok(Some((result_id, h)));
            }
            Ok(None)
        }
        OP_COMPOSITE_CONSTRUCT => {
            if o.len() >= 3 {
                let ty = ctx.ty_handle(result_ty)?;
                let comps: Vec<Handle<Expression>> = o[2..]
                    .iter()
                    .map(|&id| ctx.get_or_const_expr(func, id))
                    .collect::<Result<_, _>>()?;
                let h = func.expressions.append(Expression::Compose {
                    ty,
                    components: comps,
                });
                return Ok(Some((result_id, h)));
            }
            Ok(None)
        }
        OP_ATOMIC_IADD => {
            if o.len() >= 6 {
                let ptr = ctx.get_or_const_expr(func, o[2])?;
                let val = ctx.get_or_const_expr(func, o[5])?;
                let tmp = func.expressions.append(Expression::Literal(Literal::U32(0)));
                stmts_push_atomic(stmts, ptr, val, AtomicFunction::Add, Some(tmp))?;
                return Ok(Some((result_id, tmp)));
            }
            Ok(None)
        }
        OP_ATOMIC_ISUB => {
            if o.len() >= 6 {
                let ptr = ctx.get_or_const_expr(func, o[2])?;
                let val = ctx.get_or_const_expr(func, o[5])?;
                let tmp = func.expressions.append(Expression::Literal(Literal::U32(0)));
                stmts_push_atomic(stmts, ptr, val, AtomicFunction::Subtract, Some(tmp))?;
                return Ok(Some((result_id, tmp)));
            }
            Ok(None)
        }
        OP_PHI => Err(ParseError::Unsupported("OpPhi not supported".into())),
        OP_EXT_INST => ext_inst_glsl450(func, ctx, o, result_id),
        _ => Ok(None),
    }
}

fn stmts_push_atomic(
    stmts: &mut Vec<Statement>,
    ptr: Handle<Expression>,
    val: Handle<Expression>,
    fun: AtomicFunction,
    result: Option<Handle<Expression>>,
) -> Result<(), ParseError> {
    stmts.push(Statement::Atomic {
        pointer: ptr,
        fun,
        value: val,
        result,
    });
    Ok(())
}

fn binary(
    func: &mut Function,
    ctx: &mut FnCtx<'_>,
    o: &[u32],
    bop: BinaryOp,
) -> Result<Option<(u32, Handle<Expression>)>, ParseError> {
    if o.len() >= 4 {
        let result_id = o[1];
        let l = ctx.get_or_const_expr(func, o[2])?;
        let r = ctx.get_or_const_expr(func, o[3])?;
        let h = func.expressions.append(Expression::Binary {
            op: bop,
            left: l,
            right: r,
        });
        return Ok(Some((result_id, h)));
    }
    Ok(None)
}

fn cast(
    func: &mut Function,
    ctx: &mut FnCtx<'_>,
    o: &[u32],
    kind: ScalarKind,
) -> Result<Option<(u32, Handle<Expression>)>, ParseError> {
    if o.len() >= 4 {
        let result_id = o[1];
        let src = ctx.get_or_const_expr(func, o[3])?;
        let h = func.expressions.append(Expression::As {
            expr: src,
            kind,
            convert: Some(4),
        });
        return Ok(Some((result_id, h)));
    }
    Ok(None)
}

fn ext_inst_emit_math(
    func: &mut Function,
    ctx: &mut FnCtx<'_>,
    result_id: u32,
    fun: MathFunction,
    a: u32,
    b: Option<u32>,
    c: Option<u32>,
) -> Result<Option<(u32, Handle<Expression>)>, ParseError> {
    let arg = ctx.get_or_const_expr(func, a)?;
    let a1 = b.map(|id| ctx.get_or_const_expr(func, id)).transpose()?;
    let a2 = c.map(|id| ctx.get_or_const_expr(func, id)).transpose()?;
    Ok(Some((
        result_id,
        func.expressions.append(Expression::Math {
            fun,
            arg,
            arg1: a1,
            arg2: a2,
        }),
    )))
}

// GLSL.std.450 extended instruction opcodes (Khronos spec, section 3.32.1)
mod glsl_ext {
    pub const ROUND: u32 = 1;
    pub const ROUND_EVEN: u32 = 2;
    pub const TRUNC: u32 = 3;
    pub const F_ABS: u32 = 4;
    pub const S_ABS: u32 = 5;
    pub const F_SIGN: u32 = 6;
    pub const FLOOR: u32 = 8;
    pub const CEIL: u32 = 9;
    pub const FRACT: u32 = 10;
    pub const SIN: u32 = 13;
    pub const COS: u32 = 14;
    pub const TAN: u32 = 15;
    pub const ASIN: u32 = 16;
    pub const ACOS: u32 = 17;
    pub const ATAN: u32 = 18;
    pub const EXP: u32 = 27;
    pub const LOG: u32 = 28;
    pub const EXP2: u32 = 29;
    pub const LOG2: u32 = 30;
    pub const SQRT: u32 = 31;
    pub const INVERSE_SQRT: u32 = 32;
    pub const F_MIN: u32 = 37;
    pub const S_MIN: u32 = 39;
    pub const F_MAX: u32 = 40;
    pub const S_MAX: u32 = 42;
    pub const F_CLAMP: u32 = 43;
    pub const F_MIX: u32 = 46;
    pub const FMA: u32 = 47;
    pub const POW: u32 = 26;
    pub const ATAN2: u32 = 25;
    pub const LENGTH: u32 = 66;
    pub const DISTANCE: u32 = 67;
    pub const CROSS: u32 = 68;
    pub const NORMALIZE: u32 = 69;
    pub const FIND_I_LSB: u32 = 73;
    pub const FIND_S_MSB: u32 = 74;
    pub const FIND_U_MSB: u32 = 75;
}

fn ext_inst_glsl450(
    func: &mut Function,
    ctx: &mut FnCtx<'_>,
    o: &[u32],
    result_id: u32,
) -> Result<Option<(u32, Handle<Expression>)>, ParseError> {
    if o.len() < 5 {
        return Ok(None);
    }
    let ext_op = o[3];
    let arg0 = o.get(4).copied();
    let arg1 = o.get(5).copied();
    let arg2 = o.get(6).copied();

    let a0 = arg0.ok_or_else(|| ParseError::Unsupported("OpExtInst missing argument".into()))?;
    match ext_op {
        glsl_ext::F_ABS | glsl_ext::S_ABS => ext_inst_emit_math(func, ctx, result_id, MathFunction::Abs, a0, None, None),
        glsl_ext::F_SIGN => ext_inst_emit_math(func, ctx, result_id, MathFunction::Sign, a0, None, None),
        glsl_ext::FLOOR => ext_inst_emit_math(func, ctx, result_id, MathFunction::Floor, a0, None, None),
        glsl_ext::CEIL => ext_inst_emit_math(func, ctx, result_id, MathFunction::Ceil, a0, None, None),
        glsl_ext::FRACT => ext_inst_emit_math(func, ctx, result_id, MathFunction::Fract, a0, None, None),
        glsl_ext::ROUND | glsl_ext::ROUND_EVEN => ext_inst_emit_math(func, ctx, result_id, MathFunction::Round, a0, None, None),
        glsl_ext::TRUNC => ext_inst_emit_math(func, ctx, result_id, MathFunction::Trunc, a0, None, None),
        glsl_ext::SIN => ext_inst_emit_math(func, ctx, result_id, MathFunction::Sin, a0, None, None),
        glsl_ext::COS => ext_inst_emit_math(func, ctx, result_id, MathFunction::Cos, a0, None, None),
        glsl_ext::TAN => ext_inst_emit_math(func, ctx, result_id, MathFunction::Tan, a0, None, None),
        glsl_ext::ASIN => ext_inst_emit_math(func, ctx, result_id, MathFunction::Asin, a0, None, None),
        glsl_ext::ACOS => ext_inst_emit_math(func, ctx, result_id, MathFunction::Acos, a0, None, None),
        glsl_ext::ATAN => ext_inst_emit_math(func, ctx, result_id, MathFunction::Atan, a0, None, None),
        glsl_ext::ATAN2 => ext_inst_emit_math(func, ctx, result_id, MathFunction::Atan2, a0, arg1, None),
        glsl_ext::EXP => ext_inst_emit_math(func, ctx, result_id, MathFunction::Exp, a0, None, None),
        glsl_ext::LOG => ext_inst_emit_math(func, ctx, result_id, MathFunction::Log, a0, None, None),
        glsl_ext::EXP2 => ext_inst_emit_math(func, ctx, result_id, MathFunction::Exp2, a0, None, None),
        glsl_ext::LOG2 => ext_inst_emit_math(func, ctx, result_id, MathFunction::Log2, a0, None, None),
        glsl_ext::POW => ext_inst_emit_math(func, ctx, result_id, MathFunction::Pow, a0, arg1, None),
        glsl_ext::SQRT => ext_inst_emit_math(func, ctx, result_id, MathFunction::Sqrt, a0, None, None),
        glsl_ext::INVERSE_SQRT => ext_inst_emit_math(func, ctx, result_id, MathFunction::InverseSqrt, a0, None, None),
        glsl_ext::F_MIN | glsl_ext::S_MIN => ext_inst_emit_math(func, ctx, result_id, MathFunction::Min, a0, arg1, None),
        glsl_ext::F_MAX | glsl_ext::S_MAX => ext_inst_emit_math(func, ctx, result_id, MathFunction::Max, a0, arg1, None),
        glsl_ext::F_CLAMP => ext_inst_emit_math(func, ctx, result_id, MathFunction::Clamp, a0, arg1, arg2),
        glsl_ext::F_MIX => ext_inst_emit_math(func, ctx, result_id, MathFunction::Mix, a0, arg1, arg2),
        glsl_ext::FMA => ext_inst_emit_math(func, ctx, result_id, MathFunction::Fma, a0, arg1, arg2),
        glsl_ext::LENGTH => ext_inst_emit_math(func, ctx, result_id, MathFunction::Length, a0, None, None),
        glsl_ext::DISTANCE => ext_inst_emit_math(func, ctx, result_id, MathFunction::Distance, a0, arg1, None),
        glsl_ext::CROSS => ext_inst_emit_math(func, ctx, result_id, MathFunction::Cross, a0, arg1, None),
        glsl_ext::NORMALIZE => ext_inst_emit_math(func, ctx, result_id, MathFunction::Normalize, a0, None, None),
        glsl_ext::FIND_I_LSB => ext_inst_emit_math(func, ctx, result_id, MathFunction::FirstTrailingBit, a0, None, None),
        glsl_ext::FIND_S_MSB | glsl_ext::FIND_U_MSB => ext_inst_emit_math(func, ctx, result_id, MathFunction::FirstLeadingBit, a0, None, None),
        _ => Ok(None),
    }
}
