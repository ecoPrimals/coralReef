# Contributing to coralNak

Thank you for your interest in coralNak — a sovereign Rust NVIDIA shader compiler.

## Getting Started

```bash
# Prerequisites: Rust 1.85+ (edition 2024)
rustup update stable

# Clone and check
git clone https://github.com/ecoPrimals/coralNak.git
cd coralNak
cargo check --workspace
cargo test --workspace            # 390 tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Standards

coralNak follows ecoPrimals ecosystem conventions from `wateringHole/`.

- **License**: AGPL-3.0-only (see LICENSE). NAK-derived files retain MIT.
- **Linting**: `clippy::all` + `clippy::pedantic` + `missing_docs`, zero warnings
- **Formatting**: `cargo fmt` — no exceptions
- **Max file size**: 1000 lines (all files currently comply)
- **Test coverage**: 90%+ target (measured with `cargo llvm-cov`)
- **Unsafe**: zero `unsafe` in new code
- **Error handling**: `Result<_, CompileError>` propagation; optimizer passes skip instead of panicking
- **No `panic!` in new production code**: use `?`, `.ok_or()`, `debug_assert!`, or graceful fallback

## Architecture

See `specs/CORALNAK_SPECIFICATION.md` and `START_HERE.md`.

Key module patterns:
- **Directory modules**: Large files are split into directories (`ir/`, `from_spirv/`, `lower_f64/`, `sm70_encode/`)
- **Virtual ops**: f64 transcendentals use placeholder ops expanded by `lower_f64` before legalization
- **Pipeline**: `pipeline.rs` orchestrates the full compilation with `Result` propagation

## Commit Messages

Use conventional commits:

```
feat(compiler): add SPIR-V to NAK IR translation
fix(isa): correct SM70 DFMA latency table
refactor(stubs): replace BitSet HashSet with dense bitmap
test(core): add lifecycle error path coverage
docs(spec): update f64 lowering strategy
```

## Pull Requests

- One logical change per PR
- All checks must pass (`cargo check`, `clippy`, `fmt`, `test`, `doc`)
- Coverage must not decrease
- New public APIs require doc comments with `# Errors` / `# Panics` sections

## IPC Standards

When implementing IPC endpoints, follow `wateringHole/SEMANTIC_METHOD_NAMING_STANDARD.md`:

- Method format: `{domain}.{operation}` (e.g. `compiler.compile`, `compiler.health`)
- JSON-RPC 2.0 as primary protocol
- tarpc as optional high-performance channel
