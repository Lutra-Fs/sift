# Sift - MCP & Skills Manager

A universal MCP (Model Context Protocol) server and skills manager with GUI, TUI, and CLI interfaces.

## Features

- **Three Interface Modes**:
  - **CLI**: Command-line operations (`sift install`, `sift list`, etc.)
  - **TUI**: Terminal-based interactive interface (default when no args provided)
  - **GUI**: Native desktop application (`sift --gui`)

- **Configuration Scopes**:
  - Global: System-wide configuration
  - Per-Project Local: Project-specific, not shared
  - Per-Project Shared: Project-specific, shared across team

- **Cross-Client Compatibility**:
  - Claude Code
  - VS Code
  - Gemini CLI
  - Codex

## Installation

```bash
# Clone the repository
git clone https://github.com/lutra/sift.git
cd sift

# Build the project
cargo build --release

# The binary will be at target/release/sift
```

## Usage

```bash
# Launch TUI (default)
sift

# CLI operations
sift install mcp server-name
sift list skills
sift uninstall skill skill-name

# Set configuration scope
sift config global
sift config local
sift config shared

# Launch GUI
sift --gui
# or
sift gui
```

## Development

```bash
# Build all crates
cargo build

# Run the main binary (TUI mode)
cargo run

# Run with CLI commands
cargo run -- list mcp

# Run TUI
cargo run --bin sift-tui

# Run GUI
cargo run --bin sift-gui

# Run clippy
cargo clippy --all-targets --all-features

# Run tests
cargo test

# Format code
cargo fmt
```

## Project Structure

```
sift/
├── crates/
│   ├── sift-core/    # Core library
│   ├── sift-cli/     # CLI interface (main binary)
│   ├── sift-tui/     # TUI interface
│   └── sift-gui/     # GUI interface
```

## License

MIT OR Apache-2.0
