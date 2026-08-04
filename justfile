set shell := ["bash", "-euo", "pipefail", "-c"]

default: build

build:
    cargo build --workspace

check:
    cargo check --workspace --all-targets

test:
    cargo test --workspace

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings
