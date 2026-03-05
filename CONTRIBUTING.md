# Contributing to coralNak

Thank you for your interest in coralNak — a sovereign Rust NVIDIA shader compiler.

## Getting Started

```bash
# Prerequisites: Rust 1.85+ (edition 2024)
rustup update stable

# Clone and check
git clone https://github.com/ecoPrimals/coralNak.git
cd coralNak
cargo check
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Standards

coralNak follows ecoPrimals ecosystem conventions from `wateringHole/`.

- **License**: AGPL-3.0-only (see LICENSE). NAK-derived files retain MIT.
- **Linting**: `clippy::all` + `clippy::pedantic` + `missing_docs`, zero warnings
- **Formatting**: `cargo fmt` — no exceptions
- **Max file size**: 1000 lines for new code
- **Test coverage**: 90%+ line coverage (measured with `cargo llvm-cov`)
- **Unsafe**: zero `unsafe` in new code; audited removal in NAK-derived code
- **Error handling**: `thiserror` for libraries, proper `Result` propagation
- **No `unwrap()` in production**: use `?`, `.ok_or()`, or `.expect("reason")`

## Architecture

See `specs/CORALNAK_SPECIFICATION.md` and `START_HERE.md`.

Large NAK files have been refactored into directory modules (`ir/`, `sm70_encode/`,
`sm50/`, `sm32/`) — follow the same pattern for new encoder or IR work.

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
