# Sift

**Sift** is a configuration and dependency manager for Model Context Protocol (MCP) servers and Agent Skills.

It acts as a "bridge" between your tools and AI clients (like Claude Desktop, Claude Code, or IDEs), ensuring that the right tools are available in the right context without manual configuration hell.

## Core Philosophy

*   **Static Manager, Not Runtime Proxy**: Sift manages configuration files and downloads dependencies. It does *not* sit in the middle of the connection between the AI and the tool.
*   **Configuration as Code**: Define your toolset in `sift.toml`. Share it with your team.
*   **Scope-Aware**: Distinguish between global tools (personal), project tools (team-shared), and local overrides (secrets).

## Architecture

### 1. Configuration Scopes

Sift merges configuration from three layers to generate the final setup:

1.  **🌏 Global (User)**
    *   **Path**: `~/.config/sift/sift.toml`
    *   **Purpose**: Personal tools available everywhere (e.g., Todoist, System Info).
    *   **Git**: Ignored.

2.  **📁 Project (Shared)**
    *   **Path**: `./sift.toml` (in project root)
    *   **Purpose**: Mandatory tools for the project (e.g., Postgres connector, API Linter).
    *   **Git**: **Committed**. Ensures all team members have the same context.

3.  **🔒 Project Local (Private)**
    *   **Path**: Defined in Global config (`~/.config/sift/sift.toml`) under `[projects."/abs/path/to/project"]`.
    *   **Purpose**: Local overrides, secrets, or dev-only tools.
    *   **Git**: **N/A** (Keeps project directory clean).

### 2. Runtime Agnostic

Sift handles the complexity of heterogeneous runtimes (Node.js, Bun, Python/uv, Docker).

*   **Manifest-Driven**: Skills and MCP servers declare their requirements (e.g., "needs node >= 18").
*   **Command Generation**: Sift translates these requirements into the correct configuration for the target client (e.g., generating the correct `node` or `uv run` commands in `claude_desktop_config.json`).
*   **User Override**: Users can always override the execution command in `sift.toml` if specific runtime tweaks are needed.

## Usage

The `sift` binary provides multiple interfaces depending on your needs:

*   **No arguments**: Launches the **TUI** interface.
*   **Subcommands**: Standard **CLI** behavior (e.g., `sift install`).
*   **`sift --gui` or `sift gui`**: Launches the **GUI** interface.

### CLI Example

```bash
# Install a tool (defaults to Global)
sift install <tool-name>

# Install a tool for the current project (Shared)
sift install <tool-name> --scope project

# Generate configuration for a specific client
sift export --target claude-desktop
sift export --target vscode
```

## Roadmap

- [ ] **Core**: Config loader & merger (Rust)
- [ ] **Manifest**: Define standard schema for MCP/Skill requirements
- [ ] **Export**: Generators for Claude Desktop & VS Code
- [ ] **CLI**: Basic install/manage commands
