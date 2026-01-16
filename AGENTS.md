# Repository Guidelines

## Project Structure & Module Organization
This repo is a Rust workspace. Core logic lives under `crates/`: `sift-core` hosts domain and orchestration code, while `sift-cli`/`sift-tui`/`sift-gui` provide entrypoints. `sift-core` modules live in `crates/sift-core/src/` and are split by area (`commands/`, `orchestration/`, `config/`, `registry/`). Cross-crate integration tests are in `tests/`, and core integration tests are in `crates/sift-core/tests/`. Shared configuration lives in `sift.toml`, and design/reference docs are in `design.md`, `mcp-docs/`, and `skill-docs/`.

## Build, Test, and Development Commands
- `cargo build`: Build the entire workspace.
- `cargo test`: Run all tests.
- `cargo test -p sift-core`: Run only core tests.
- `cargo run -p sift-cli -- <args>`: Run the CLI locally.
- `cargo run -p sift-tui` / `cargo run -p sift-gui`: Run the TUI/GUI.

## Coding Style & Naming Conventions
Follow Rust 2024 edition conventions and use `rustfmt` and `clippy`. Prefer `snake_case`; keep type and module names clear and concise. Organize code by area under `crates/sift-core/src/<area>/` and avoid cross-layer dependencies. Before committing, run:
```bash
cargo fmt
cargo clippy --all-targets --all-features
```

## Testing Guidelines
Tests use the Rust test framework with `#[test]` and `#[tokio::test]`. Prefer behavior tests in `crates/sift-core/tests/` with descriptive filenames (e.g., `install_command.rs`, `*_spec.rs`). New features should cover main paths and error branches; add boundary cases for parsing or config merging.

## Commit & Pull Request Guidelines
Commit messages follow Conventional Commits (e.g., `feat: ...`, `refactor(scope): ...`, `fix: ...`). PRs should include a summary, motivation/impact, and the test commands run. For TUI/GUI changes, include screenshots or recordings; link related issues when applicable.

## Architecture & Docs Notes
Design assumptions and behavior contracts live in `design.md`; protocol and our supported client docs are in `mcp-docs/` and `skill-docs/`. Review these before making changes that could affect semantics.

## Configuration & Safety Tips
Local user config defaults to dirs::config_dir, while the repo `sift.toml` is shared. When adding config fields, update parsing and schema and consider lockfile and merge behavior.

Follow the articles described below when generating code.

The most transformative article—no code before tests:

```text
This is NON-NEGOTIABLE: All implementation MUST follow strict Test-Driven Development.
No implementation code shall be written before:
1. Unit tests are written
2. Tests are validated and approved by the user
3. Tests are confirmed to FAIL (Red phase)
```

This completely inverts traditional AI code generation. Instead of generating code and hoping it works, the LLM must first generate comprehensive tests that define behavior, get them approved, and only then generate implementation.

If you were asked to fix a bug, you must think on why the bug happened, and the existing tests did not catch it. You must write new tests that would have caught the bug before fixing it.

## Repo Guidelines
The design docs are in @design.md.
Referenced client's docs are in @mcp-docs and @skill-docs.
For any .unwrap usage, pls do explore the code flow first, and ensure that it is safe to use .unwrap without risk of panics. You must write a comment to justify the usage of .unwrap.
DO NOT Think on any backward compatibility unless explicitly asked by the user. Since we are in early development phase, we can break backward compatibility as needed, if the architecture/design needs it or it is better.
If you need to ask me for any clarifications on behavior, pls also check how other package managers (like npm, pip, cargo, etc) behave with similar senario and give me a report first.

## Architecture Reminders
* Clients are **plan-only adapters**—they describe `ManagedJsonPlan`/`SkillDeliveryPlan` and capabilities (MCP/Skills scope support plus symlink allowance). The core `InstallOrchestrator` executes all filesystem writes and enforces ownership/lockfile tracking.
* Scope enforcement distinguishes explicit vs implicit intent: explicit requests that reference an unsupported scope must error; implicit/all-target installs warn+skip those clients. Skills `local` scope is allowed only in Git repos, delivered via project scope plus updating `.git/info/exclude`; MCP `local` only when the client explicitly supports it.
* Ownership lives inside the lockfile (`Lockfile.managed_configs`); no separate ownership files. `InstallOrchestrator` updates the lockfile whenever managed JSON entries or skills change.
* `link_mode` is now a **global policy** sourced from `SiftConfig.link_mode`. Symlink mode downgrades automatically (`Symlink → Hardlink → Copy`) when a client lacks symlink capability.
* MCP transport surface is currently limited to `stdio`/`http`; SSE is intentionally absent to keep the configuration deterministic.
