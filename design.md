# **Sift: A local MCP & Agent Skills Manager**

## **Part I: Core Model**

### **1. Philosophy**
Sift is **not a runtime proxy**. It does not intercept traffic between your agent and tools.
Sift is a **Configuration Generator** and **Package Manager**.

*   **The Problem**: Configuration Hell. Manually editing each coding agent's config (Claude Desktop, VS Code, etc.) is error-prone and decentralized.
*   **The Solution**: Sift maintains its own source-of-truth configuration (`sift.toml`) and "compiles" it into the format required by target clients.

### **2. Configuration Scopes (The 3-Layer Model)**
Sift merges configurations in the following order (lower overrides higher):

1.  **Layer 1: 🌏 Global (User)**
    *   **Location**: `~/.config/sift/sift.toml`
    *   **Use Case**: Personal workflow tools (Calendar, Notes, OS interaction).
    *   **Git**: Not version controlled.

2.  **Layer 2: 📁 Project (Shared)**
    *   **Location**: `./sift.toml` (Project Root).
    *   **Use Case**: Tools required for the project (DB connectors, Linters).
    *   **Git**: **COMMITTED**. Ensures "Checkout & Run" experience for teams.

3.  **Layer 3: 🔒 Project Local (Private)**
    *   **Location**: Defined within `~/.config/sift/sift.toml` under `[projects."/abs/path/to/project"]`.
    *   **Use Case**: Secrets (API Keys), developer-specific tools, runtime overrides.
    *   **Git**: **N/A** (Stored entirely in your global user config).

### **3. Configuration Schema (`sift.toml`)**

```toml
# --- Tool Definitions (Valid in Global & Project) ---

[mcp.postgres]
source = "registry:postgres-mcp"
runtime = "docker"
targets = ["claude-desktop", "vscode"] # Whitelist
# ignore_targets = ["codex"]           # Blacklist

[mcp.postgres.env]
DB_URL = "postgres://..."

[skill.pdf-processing]
source = "registry:anthropic/pdf"

# --- Private Overrides (Valid ONLY in Global ~/.config/sift/sift.toml) ---

[projects."/Users/me/repos/my-app".mcp.postgres.env]
DB_URL = "postgres://user:pass@localhost:5432/mydb"
```

### **4. Safety & Ownership**
Sift follows a strict "Do No Harm" policy regarding configuration files.

*   **Provably Sift-Managed**: Sift only modifies entries it created (tracked via hash/lockfile).
*   **User Precedence**: If an existing entry in a client config differs from the last Sift render, it is treated as **user-modified** and preserved verbatim unless forced.
*   **Non-Managed Entries**: Preserved verbatim.

---

## **Part II: Managed Resources**

### **5. MCP Servers**

#### **Transports**
1.  **STDIO (Default)**: Sift launches the process.
    *   Configuration: `runtime`, `args`, `env`.
2.  **HTTP**: Connects to an existing endpoint.
    *   Configuration: `url`, `headers` (supports `${VAR}` expansion).
3.  **SSE**: Intentionally excluded to keep configuration deterministic.

#### **CLI Explicit Install**
*   Explicit command or URL has the highest priority.
*   When user provides stdio command (`--transport stdio -- <command>`) or HTTP URL (`--transport http --url <url>`), Sift writes configuration directly without registry resolution.
*   If `--source`, `--registry`, or `name@version` is provided simultaneously, CLI will ignore these parameters and issue a warning.
*   If `--runtime` is provided with an explicit command or URL, CLI will ignore it and issue a warning.
*   When only `--env`/`--header` are provided, registry resolution logic is still used.

#### **Heterogeneous Runtimes**
Sift treats runtimes as first-class citizens.
*   **Manifest Strategy**: Registries define defaults (e.g., `default_runtime = "docker"`).
*   **User Overrides**: Users can override `runtime` in `sift.toml`.

#### **Runtime Isolation**
To avoid polluting the system PATH, Sift enforces cache isolation for runtimes:
*   **bunx**: Uses `--cache-dir <sift-cache>`.
*   **npx**: Uses `npm_config_cache=<sift-cache>`.
*   Generated configurations embed these cache settings in environment variables.

### **6. Agent Skills**

#### **Storage & Distribution**
*   **Central Cache**: Skills are downloaded to **XDG Data Home** (`~/.local/share/sift/skills/`), organized by `registry/author/skill-name/version`.
*   **Link Mode (`link_mode`)**: Defines how skills are exposed to clients.
    *   **Global Policy**: Set in `sift.toml`.
    *   **Downgrade Strategy**: If a client lacks capability (e.g., doesn't support symlinks), Sift automatically downgrades: `Symlink → Hardlink → Copy`.

#### **Naming & Source Inference**
*   **Directory Name as Identity**: Sift treats the directory name as the canonical skill name and does not read `SKILL.md` during install. Skill authors should keep `SKILL.md` aligned with the directory name.
*   **Local/Git Auto-Detect**: When user provides local path or Git URL, CLI automatically infers `source` as `local:` or `git:`, no longer requiring `--source`.
*   **Registry Disambiguation**: If multiple registries provide a skill or MCP with the same name and user doesn't explicitly specify `--registry` (or a `registry:` source), CLI will warn and require user to make an explicit choice.
*   **Version Declaration**: Only `name@version` is supported for expressing version constraints (no longer provides `--version`), which is parsed and written to config as declared version.
*   **Git Requirement**: Installing skills from Git URLs requires Git 2.25+ for sparse checkout.

#### **Lifecycle: Ejection**
Allows users to modify a managed skill by converting it to a local project file.

1.  **Eject (`sift skill eject <name>`)**:
    *   Deletes the Symlink.
    *   **Copies** the skill directory from central cache to `./skills/<name>`.
    *   Updates `sift.toml` source to `"local:..."`.
    *   *Result*: Skill is now user-managed and stops receiving updates.

2.  **Un-eject (`sift skill un-eject <name>`)**:
    *   **Safety Check**: Verifies if local directory is "clean" (git status).
    *   **Backup**: Moves local directory to `.sift/ejected-backups/<name>/<timestamp>`.
    *   **Restore**: Reverts `sift.toml` to registry source and restores symlink.

---

## **Part III: The Engine**

### **7. Registry & Discovery**
Sift separates **Fetch** (I/O) from **Adapt** (Transform) to handle heterogeneous sources.

*   **Registry Adapters**:
    *   `type = "sift"`: Native, optimized format (Rich metadata, deps).
    *   `type = "claude-marketplace"`: Adapts `marketplace.json` from Claude Code ecosystem.
*   **Resolver**: Maps user queries to concrete sources using the **Declared vs Resolved** policy.

### **8. Versioning & Locking**

#### **Philosophy: Reproducibility > Freshness**
*   **Install-Time Snapshot**: `sift add` resolves the **latest** version and **locks** it immediately.
*   **Explicit Upgrade**: Updates only happen on `sift upgrade`.
*   **Registry Capabilities**: Registry implementation declares whether it supports historical versions; when not supported, a warning is issued for `name@version` and the version is ignored.

#### **Lockability Matrix**
| Source | Lockable? | Resolution Strategy |
| :--- | :--- | :--- |
| **Remote MCP (HTTP)** | No | Config-only, floating. |
| **Local MCP (npm/bun)** | Partial | Writes resolved version snapshot. |
| **Local MCP (Docker)** | Yes | Resolves to Digest. |
| **Skills (Git)** | Yes | Resolves to Commit SHA. |

#### **Lockfile Semantics**
*   **Source of Truth**: `sift.toml` holds the version constraint and the resolved snapshot.
*   **Install State**: The Lockfile (`sift.lock`) tracks physical install details (cache path, tree hash, delivery mode).

### **9. Install Pipeline**
1.  **Resolve**: `RegistryResolver` finds package.
2.  **Version**: `VersionResolver` determines specific SHA/Digest.
3.  **Config**: Update `sift.toml`.
4.  **Execute**:
    *   **Skills**: Fetch -> Verify Tree Hash -> Link to Targets -> Update Lockfile.
    *   **MCP**: Generate RunnerSpec -> Generate Client Configs (JSON).

#### **Cache Optimization**
If a skill cache exists, Sift validates the **Tree Hash**.
*   Match: Link directly (no network).
*   Mismatch: Fail (unless `--force`).

---

## **Part IV: Integration**

### **10. Client Adapters**
Adapters bridge Sift configuration to external tools (Claude Desktop, VS Code).

*   **Plan-Only**: Adapters describe desired states (JSON paths, file links) but **do not** perform I/O.
*   **Capabilities Interface**:
    1.  **Scope Support**: Does the client support Project/Local scopes?
    2.  **Skill Delivery**: `Filesystem` (scan dir) vs `ConfigReference` (explicit paths) vs `None`.
    3.  **MCP Compatibility**: Transport support, header support, config format.

### **11. Scope Enforcement**
1.  **Explicit Targets (`targets = ["app"]`)**: **Fail-Fast**. If the client doesn't support the current scope, error out.
2.  **Implicit Targets (Empty/All)**: **Best-Effort**. Skip unsupported clients with a warning.

### **12. Scope Boundary: Claude Code Plugins**
Sift does **not** manage plugins installed directly inside Claude Code. Sift only manages what is defined in `sift.toml`.

---

## **Part V: User Interface**

### **13. CLI Workflow**
Sift uses a **Resource-Oriented** design (`noun verb`) with shortcuts.

#### **Common Commands**
*   **Setup**: `sift init`, `sift install` (alias `add`).
*   **Maintenance**: `sift status`, `sift upgrade`, `sift apply`.
*   **Cleanup**: `sift uninstall` (alias `rm`).
*   **Inspection**: `sift list` (alias `ls`), `sift doctor`.
*   **Resource based**: `sift mcp <verb>`, `sift skill <verb>`, `sift registry <verb>`.

#### **Install Input Rules**
*   If the install target looks like a local path, Sift treats it as local and normalizes to `local:`.
*   If it looks like a git URL, Sift normalizes to `git:`.
*   Otherwise, Sift treats the input as a registry package name.
*   If multiple registries are configured and the user provides a bare name, CLI requires `--registry`.
*   `--source` accepts fully-qualified sources and will be normalized when a raw URL or path is provided (warning on normalization).

#### **Orphaned Entries & Pruning**
When a tool is removed from `sift.toml` but remains in the lockfile or filesystem, it is **Orphaned**.

*   **Detection**: `sift status` marks these as `Orphaned`.
*   **Cleanup**: `sift install --prune`.

```bash
$ sift status --scope global
Skills (2):
┌────────────┬───────────┐
│ Name       │ State     │
├────────────┼───────────┤
│ good-skill │ Ok        │
│ old-skill  │ Orphaned  │
└────────────┴───────────┘

$ sift install --prune
Removing 1 orphaned entry:
  - old-skill (Global)
```

### **14. System Diagnostics (`sift doctor`)**
Validates the environment:
1.  **Runtimes**: Checks for `uv`, `node` (suggests `bun` fallback), `docker`.
2.  **Environment**: WSL checks, permission checks.
3.  **Connectivity**: Registry reachability.
4.  **Git**: Validate `git` is installed and version >= 2.25 for Git skill installs.

### **15. Security & Trust**
*   **No Sandbox**: Skills run with user privileges.
*   **Transparency**: CLI displays full source URLs before install.
*   **Recommendation**: Prefer `docker` runtime for MCP servers where possible.
