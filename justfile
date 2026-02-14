# fmm-bench development commands

# Run all checks (clippy + format check)
check:
    cargo clippy --all-targets -- -D warnings
    cargo fmt -- --check

# Build the project
build:
    cargo build

# Run all tests
test:
    cargo test

# Format code
fmt:
    cargo fmt

# Run a quick validation
validate: check build test
