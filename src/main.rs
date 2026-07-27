//! Thin shell entrypoint. The real work lives in the `dotcfg_gui` library
//! crate; this binary only chooses between a no-GUI exit (for unit-test CI
//! jobs that run the unit tests on a headless machine) and the GTK shell
//! entrypoint.

fn main() {
    #[cfg(not(feature = "gtk"))]
    {
        println!(
            "dotcfg-gui core built without the `gtk` feature (so no GUI). \
             Build with `--features gtk` for the GUI shell. Exiting 0."
        );
    }

    #[cfg(feature = "gtk")]
    {
        if let Err(e) = dotcfg_gui::run_shell(vec![]) {
            eprintln!("dotcfg-gui: shell error: {}", e);
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    /// Placeholder so `cargo nextest run --no-default-features` does not exit
    /// with NO_TESTS_RUN on Wave 0 Step 2. Real test targets live in `tests/`;
    /// the first integration target is `tests/undo_stack_round_trip.rs`.
    #[test]
    fn nextest_smoke_discovery() {}
}
