# Skill Docket App — dev recipes

default:
    @just --list

# Build all crates (release)
build: check
    cargo build --release

# Build debug
build-debug: check
    cargo build

# Run all tests
test:
    cargo test

# Run only the hollow world E2E scenarios
test-hw:
    cargo test --lib hw_scenario

# Run tests for a specific crate
test-core:
    cargo test -p skill-docket-core

test-cli:
    cargo test -p skd-cli

test-tui:
    cargo test -p skd-tui

# Install skd binary to ~/bin
install: build
    @mkdir -p ~/bin
    @ln -sf "$(pwd)/target/release/skd" ~/bin/skd
    @echo "Installed: ~/bin/skd → target/release/skd"

# Uninstall from ~/bin
uninstall:
    @rm -f ~/bin/skd
    @echo "Removed ~/bin/skd"

# Start the daemon in foreground
daemon:
    cargo run --release -p skd-cli -- daemon run

# Stop the running daemon
daemon-stop:
    cargo run --release -p skd-cli -- daemon stop

# Show system status
status:
    cargo run --release -p skd-cli -- status

# Dev environment check
check:
    @command -v cargo >/dev/null || { echo "ERROR: cargo not found"; exit 1; }
    @command -v rustc >/dev/null || { echo "ERROR: rustc not found"; exit 1; }

# Clean build artifacts
clean:
    cargo clean

# Quick pre-commit gate (fast tests only)
test-commit:
    cargo test --lib -p skill-docket-core -- --quiet
