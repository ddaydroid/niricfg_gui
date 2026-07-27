// dotcfg-gui — extensible GTK4/libadwaita dotfile config editor (v0.1.0-stub).
// Real implementation lands in subsequent commits per the v0.0.0-spec plan at
// .specs/tasks/todo/implement-dotcfg-gui.feature.md (Step 1 of 22).

fn main() {
    println!("dotcfg-gui v0.1.0-stub; full implementation lands in subsequent commits.");
}

#[cfg(test)]
mod tests {
    /// Smoke test: confirms the binary crate compiles under `cargo test`.
    /// Real tests arrive in Wave 0 Step 2 (ToolPlugin trait + UndoStack + shell scaffolding).
    /// Exists to keep `cargo nextest run --no-default-features` from exiting with
    /// NO_TESTS_RUN (exit code 4) on the stub commit.
    #[test]
    fn main_compiles() {}
}
