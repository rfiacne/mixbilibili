# Contributing to mixbilibili

Thank you for your interest in contributing!

## Development Setup

1. Install Rust via [rustup](https://rustup.rs/)
2. Clone the repository
3. Run `cargo test` to verify your setup

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy` and fix all warnings
- Add tests for new functionality

## Pull Requests

1. Create a feature branch from `master`
2. Make your changes with clear commit messages
3. Ensure CI passes (format, clippy, tests)
4. Open a pull request with a description of changes

## Testing

```bash
# Run all tests
cargo test

# Run tests for a specific module
cargo test scanner
cargo test merger
cargo test cli

# Run clippy
cargo clippy -- -D warnings

# Check formatting
cargo fmt --check
```

## Release Process

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Create a git tag: `git tag v0.x.x`
4. Push tag: `git push --tags`
5. GitHub Actions will build and publish releases