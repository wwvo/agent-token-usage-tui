# justfile — common project tasks
#
# Run `just` with no arguments to list every recipe. Each task mirrors
# what CI does; staying close to CI locally means "works on my machine"
# and "works on the runner" converge.
#
# Requires: cargo (stable or nightly works), just >= 1.14.

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# Show every recipe and its first-line comment.
default:
    @just --list --unsorted

# ---- Formatting ----------------------------------------------------------

# Format the entire workspace. Use before committing.
fmt:
    cargo fmt

# Fail if anything is unformatted. Intended for CI / pre-push checks.
fmt-check:
    cargo fmt --check

# ---- Lints ---------------------------------------------------------------

# Strict clippy: treat warnings as errors across every target (bins +
# tests + examples). Matches the CI lint gate exactly so local runs
# catch the same issues.
clippy:
    cargo clippy --all-targets -- -D warnings

# ---- Tests ---------------------------------------------------------------

# Run all tests (lib + doc + integration).
test:
    cargo test

# Same, but suppress `println!`/passing-test noise. Good when you only
# care about the failure output.
test-quiet:
    cargo test --quiet

# Regenerate and serve cargo doc for the crate, no dependency docs.
doc:
    cargo doc --no-deps --open

# ---- Dev runs ------------------------------------------------------------

# Run the CLI with arbitrary arguments (everything after `just run` is
# forwarded). Examples:
#     just run --help
#     just run scan
#     just run tui
run *ARGS:
    cargo run -- {{ARGS}}

# Shortcut for the most common command.
scan:
    cargo run -- scan

# Shortcut for the TUI (note: cargo run eats raw-mode output cleanly).
tui:
    cargo run -- tui

# ---- Release build -------------------------------------------------------

# Build the optimized, stripped, single-binary release (dist profile).
# Outputs to target/dist/atut(.exe).
release:
    cargo build --profile dist --locked

# ---- CI parity -----------------------------------------------------------

# Run the same gates CI will run. Succeed locally before pushing.
ci: fmt-check clippy test
