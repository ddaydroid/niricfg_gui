# Integration tests

This directory is reserved for the integration test targets that will land in
subsequent commits per the Wave 0 Step 2 plan at
`.specs/tasks/todo/implement-dotcfg-gui.feature.md` (Step 2 of 22).

## Layout convention

Each integration test target lives as its own `.rs` file in this directory.
**Cargo auto-discovers every top-level `.rs` file in `tests/` and compiles each
one as its own test binary**, which `cargo test` and `cargo nextest` then
enumerate. This means a single `tests/tool_plugin_discovery.rs` becomes the
test binary `tool_plugin_discovery`.

FUTURE targets (not yet committed):

| File                              | Wave / Spec Step |
|-----------------------------------|------------------|
| `undo_stack_round_trip.rs`        | Wave 1 / Step 7  |
| `tool_plugin_discovery.rs`        | Wave 1 / Step 8  |
| `kdl_round_trip.rs`               | Wave 2 / Step 10 |

## Rules

- **Do NOT** create `tests/main.rs` or `tests/lib.rs` — those names are
  reserved for cargo's own `main` / `lib` target logic. Use descriptive,
  per-target names instead (above).
- Tests should default to `--no-default-features` so they run under the
  `Tests (Debug)` CI job's `Unit tests (no GTK)` step (which uses
  `cargo nextest run --no-default-features`).
- Tests that require GTK system libraries must be gated on
  `#[cfg(feature = "gtk")]`, so that they only run under the
  `Integration tests (xvfb)` step (`xvfb-run -a cargo nextest run --features gtk`).
- Use `tempfile::tempdir()` (already a dev-dependency in `Cargo.toml`) for any
  filesystem fixtures — never write under `$TMPDIR` directly.
- Use `proptest! { … }` (also a dev-dependency) for property-based cases, gated
  on `#[cfg(feature = "kdl")]`. The `PROPTEST_CASES=2000` env var on the
  `Property Tests (Release)` job will scale the iteration count automatically.

## How to run locally

```sh
# all unit + integration tests under default features
cargo nextest run --no-default-features

# all unit + integration tests under gtk feature (needs libgtk-4-dev on host)
xvfb-run -a cargo nextest run --features gtk --no-fail-fast
```

## Status

Empty by design as of commit `27ee843` (the `nextest_smoke_discovery` placeholder in
`src/main.rs` keeps `cargo nextest` happy until the first real target lands here).
