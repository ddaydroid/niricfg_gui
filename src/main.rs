// dotcfg-gui — extensible GTK4/libadwaita dotfile config editor (v0.1.0-stub).
// Real implementation lands in subsequent commits per the v0.0.0-spec plan at
// .specs/tasks/todo/implement-dotcfg-gui.feature.md (Step 1 of 22).

fn main() {
    println!("dotcfg-gui v0.1.0-stub; full implementation lands in subsequent commits.");
}

#[cfg(test)]
mod tests {
    /// Placeholder test target so `cargo nextest run --no-default-features` does
    /// not exit with NO_TESTS_RUN (exit code 4) on the stub commit. Real test
    /// targets live under `tests/` per the integration-test layout convention
    /// documented in `tests/README.md`; the first ones land in Wave 0 Step 2 of
    /// `.specs/tasks/todo/implement-dotcfg-gui.feature.md`
    /// (ToolPlugin discovery + UndoStack round-trip).
    #[test]
    fn nextest_smoke_discovery() {}
}
