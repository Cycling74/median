//! The errors of `#[derive(MaxObject)]` and `#[median::methods]`: each file in `tests/ui` must
//! fail to compile with the error in its `.stderr` file. (`TRYBUILD=overwrite` regenerates them.)

#[test]
fn macro_errors() {
    //trybuild runs cargo: cap its jobs like the rest of the repository (half the logical cores)
    //unless the caller has
    if std::env::var_os("CARGO_BUILD_JOBS").is_none() {
        let jobs = std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1));
        // SAFETY: this is the only test in this binary, so no other thread reads the
        // environment meanwhile
        unsafe { std::env::set_var("CARGO_BUILD_JOBS", jobs.to_string()) };
    }
    //a rustc wrapper may rewrite the paths in diagnostics (kache does), which would make the
    //expected output machine specific: build these without one. An empty value overrides a
    //wrapper set in cargo's config.
    // SAFETY: as above
    unsafe { std::env::set_var("RUSTC_WRAPPER", "") };
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
