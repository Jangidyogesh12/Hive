# Contributing

Thanks for helping improve Hive. This project is pre-`v0.1.0`, so APIs and storage details can still change.

## Development Setup

Install the stable Rust toolchain with `rustfmt` and `clippy`.

Useful commands:

```bash
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo doc --workspace --no-deps
```

For normal development, run `cargo test --workspace`. The engine integration tests live in `testing/core` under the `hive_core_testing` crate.

## Before Opening A PR

- Keep changes focused and minimal.
- Add or update tests for behavior changes.
- Update docs when public behavior, commands, query syntax, or storage format changes.
- Run formatting, clippy, and tests locally.
- Do not commit generated database directories such as `.hive/`.

## Code Style

- Prefer small, direct implementations over broad abstractions.
- Keep storage changes explicit about durability and recovery behavior.
- Do not suppress clippy warnings unless there is a clear reason documented in code.
- Use `DbError` for engine errors exposed through public APIs.

## Commit Messages

Use concise, imperative messages that describe the purpose of the change, for example:

```text
add CI quality checks
fix WAL recovery replay ordering
document supported Cypher subset
```

## License

By contributing, you agree that your contributions are licensed under the MIT license used by this repository.
