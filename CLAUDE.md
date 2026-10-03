
# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`log_analyzer` is a Rust binary crate, currently a fresh scaffold (hello-world `main`, no dependencies, no tests yet). Rust edition 2024, so newer language features (let-chains, `gen` blocks, etc.) are available.

## Commands

- Build: `cargo build`
- Run: `cargo run`
- Run all tests: `cargo test`
- Run a single test: `cargo test <test_name>`
- Run tests with output visible: `cargo test -- --nocapture`
- Lint: `cargo clippy` (treat warnings as errors when fixing code: `cargo clippy -- -D warnings`)
- Format: `cargo fmt` (check only: `cargo fmt --check`)

## Structure

Single binary crate: all code lives under `src/` with the entry point in `src/main.rs`. No module layout has been established yet — as the project grows, prefer splitting into modules under `src/` with unit tests colocated (`#[cfg(test)] mod tests`) and integration tests in `tests/`.
