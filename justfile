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

# ---- Windsurf exporter (.vsix) ------------------------------------------
#
# Companion VSCode extension lives under `tools/windsurf-exporter/`. The
# two recipes below wrap its npm scripts so you don't have to cd around.
# `npm ci --loglevel=error` is used (not `npm install`) to keep installs
# reproducible against the committed package-lock.json and to mute the
# usual deprecation / warning noise that doesn't block compilation.
#
# Both recipes use `#!/usr/bin/env bash` shebangs for the same reason as
# release-prepare / release-publish above: Windows `powershell` (PS 5.x)
# doesn't support `&&` chaining and bash ships with Git for Windows.
#
# Requires: Node.js >= 18, npm on PATH.

# Type-check the exporter without emitting a .vsix. Fast (~10s after
# cache), surfaces TypeScript errors before they hit CI or a user who
# just ran `just vsix`.
vsix-check:
    #!/usr/bin/env bash
    set -euo pipefail
    cd tools/windsurf-exporter
    npm ci --loglevel=error
    npm run compile

# Build the installable `agent-token-usage-tui-windsurf-exporter-X.Y.Z.
# vsix`. `npm run package` delegates to `vsce package --no-dependencies`
# (we bundle zero runtime deps, so no vendoring work needed). The output
# `.vsix` lands in `tools/windsurf-exporter/`; install it in Windsurf via
# "Extensions → … → Install from VSIX".
vsix:
    #!/usr/bin/env bash
    set -euo pipefail
    cd tools/windsurf-exporter
    npm ci --loglevel=error
    npm run package

# ---- CI parity -----------------------------------------------------------

# Run the same gates CI will run. Succeed locally before pushing.
ci: fmt-check clippy test

# ---- Release publish (bump + tag + push) --------------------------------
#
# These two recipes drive the full release flow end-to-end. Split in
# half on purpose so you can eyeball the bump locally before pushing:
#
#     just release-prepare 0.2.4   # bumps + commits + tags locally
#     git log -2 --stat            # inspect the bump commit
#     just release-publish         # pushes main + tag to both remotes
#
# Both recipes use `#!/usr/bin/env bash` shebangs so they bypass the
# `set windows-shell := powershell` above and run under Git-for-Windows'
# bundled bash on Windows — the sed / git-cliff / loop syntax below is
# POSIX, not PowerShell.

# Prepare a new release locally: bump Cargo.toml + Cargo.lock to the
# given version, regenerate CHANGELOG.md via git-cliff (filing every
# unreleased commit under the new tag's section), commit the bump as
# `chore(release)` (which `cliff.toml` skips from the changelog), and
# cut an annotated `v<version>` tag. Does NOT push.
#
# Preconditions:
#   * Working tree is clean (no uncommitted changes).
#   * Tag `v<version>` does not already exist locally.
#   * `git-cliff` is on PATH (scoop install git-cliff).
#
# Usage:
#     just release-prepare 0.2.4
release-prepare version:
    #!/usr/bin/env bash
    set -euo pipefail

    tag="v{{version}}"

    if [[ -n "$(git status --porcelain)" ]]; then
        echo "error: working tree has uncommitted changes; clean it first" >&2
        exit 1
    fi
    if git rev-parse -q --verify "refs/tags/${tag}" >/dev/null; then
        echo "error: tag ${tag} already exists locally" >&2
        exit 1
    fi

    echo "Bumping Cargo.toml / Cargo.lock to {{version}}…"
    # [package].version is the first `^version = ` line; the sed
    # address range `0,/re/` targets only that first match.
    sed -i '0,/^version = /{s/^version = ".*"/version = "{{version}}"/}' Cargo.toml
    # Cargo.lock has exactly one `agent-token-usage-tui` entry; the
    # `{n;...}` trick moves to the line after `name = ...` (the
    # version line of the same [[package]] block) and rewrites it.
    sed -i '/^name = "agent-token-usage-tui"$/{n;s/^version = ".*"/version = "{{version}}"/}' Cargo.lock

    echo "Regenerating CHANGELOG.md with ${tag}…"
    git cliff --tag "${tag}" -o CHANGELOG.md

    echo "Creating release commit + ${tag} tag…"
    git add Cargo.toml Cargo.lock CHANGELOG.md
    git commit -m "🔖 chore(release): bump {{version}}" \
               -m "- Cargo.toml + Cargo.lock: version bump to {{version}}" \
               -m "- CHANGELOG.md: regenerated by git-cliff (new [{{version}}] section)"
    git tag -a "${tag}" \
            -m "${tag}" \
            -m "Release notes are generated on the CI side; see the Release page for the auto-generated changelog + contributors list."

    echo
    echo "✅ Prepared ${tag} locally. Inspect before publishing:"
    echo "   git log -2 --stat"
    echo "   git show --stat ${tag}"
    echo
    echo "When satisfied, run:  just release-publish"

# Push main + the tag at HEAD to both remotes (cnb.cool = origin,
# github.com = github). Refuses to run unless HEAD is exactly at an
# annotated tag — pairs with `release-prepare` which leaves HEAD there.
#
# Usage:
#     just release-publish
release-publish:
    #!/usr/bin/env bash
    set -euo pipefail

    tag="$(git describe --tags --exact-match HEAD 2>/dev/null || true)"
    if [[ -z "${tag}" ]]; then
        echo "error: HEAD is not at a tag; run 'just release-prepare <version>' first" >&2
        exit 1
    fi

    for remote in origin github; do
        echo "Pushing main + ${tag} to ${remote}…"
        git push "${remote}" main
        git push "${remote}" "${tag}"
    done

    echo
    echo "✅ ${tag} published. Monitor pipelines:"
    echo "   - cnb:    https://cnb.cool/prevailna/agent-token-usage-tui/-/build/logs"
    echo "   - github: https://github.com/wwvo/agent-token-usage-tui/actions"
