//! Binary entry point.
//!
//! M1 bootstrap: prints the package name and version so that `cargo run`
//! produces a well-defined string during the skeleton phase. Later commits
//! replace this with a full CLI dispatcher (see `phase1-skeleton-commits.md`).

use std::io::Write;

fn main() -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )?;
    Ok(())
}
