//! Sift - MCP & Skills Manager
//!
//! Usage:
//!   sift              # Launch TUI (default)
//!   sift status       # Show installation status
//!   sift install ...  # CLI operations
//!   sift --gui        # Launch GUI

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use sift_core::commands::{InstallCommand, InstallOptions, InstallTarget};
use sift_core::config::ConfigScope;
use sift_core::status::{EntryState, McpServerStatus, SkillStatus, SystemStatus, collect_status};

#[derive(Parser)]
#[command(name = "sift")]
#[command(about = "MCP & Skills Manager", long_about = None)]
struct Cli {
    /// Launch GUI interface
    #[arg(long, short)]
    gui: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show installation status
    Status {
        /// Filter by scope (global, shared, local)
        #[arg(long)]
        scope: Option<String>,

        /// Show global scope only (alias for --scope global)
        #[arg(short = 'g', long)]
        global: bool,

        /// Verify file integrity (slower, checks hashes)
        #[arg(long)]
        verify: bool,

        /// Output format
        #[arg(short, long, default_value = "table")]
        format: OutputFormat,

        /// Show per-client deployment details
        #[arg(short = 'v', long)]
        verbose: bool,
    },

    /// Install an MCP server or skill
    Install {
        /// What to install (mcp or skill)
        kind: String,
        /// Name/ID of the package to install
        name: String,
        /// Source specification (e.g., "registry:name" or "local:/path")
        #[arg(long, short)]
        source: Option<String>,
        /// Configuration scope (global, shared, local)
        #[arg(long)]
        scope: Option<String>,
        /// Force overwrite existing entries
        #[arg(long, short)]
        force: bool,
        /// Runtime type for MCP servers (node, bun, docker, etc.)
        #[arg(long, short)]
        runtime: Option<String>,
        /// Transport type for MCP servers (stdio or http)
        #[arg(long)]
        transport: Option<String>,
        /// HTTP URL for MCP servers
        #[arg(long)]
        url: Option<String>,
        /// Environment variable for MCP servers (KEY=VALUE)
        #[arg(long, value_name = "KEY=VALUE")]
        env: Vec<String>,
        /// HTTP header for MCP servers (KEY=VALUE)
        #[arg(long, value_name = "KEY=VALUE")]
        header: Vec<String>,
        /// Stdio command for MCP servers (after --)
        #[arg(last = true)]
        command: Vec<String>,
    },

    /// Uninstall an MCP server or skill
    Uninstall {
        /// What to uninstall (mcp or skill)
        kind: String,
        /// Name/ID of the package to uninstall
        name: String,
    },

    /// List MCP servers or skills
    List {
        /// What to list (mcp or skill)
        kind: Option<String>,
    },

    /// Set configuration scope
    Config {
        /// Configuration scope (global, local, shared)
        scope: String,
    },
}

#[derive(Clone, Copy, ValueEnum, Default)]
enum OutputFormat {
    /// Human-readable table
    #[default]
    Table,
    /// Machine-readable JSON
    Json,
    /// Only show issues (non-zero exit if problems)
    Quiet,
}

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sift=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();

    // Route to appropriate interface
    if cli.gui {
        run_gui();
    } else if let Some(command) = cli.command {
        run_cli(command)?;
    } else {
        run_tui()?;
    }

    Ok(())
}

fn run_cli(command: Commands) -> Result<()> {
    match command {
        Commands::Status {
            scope,
            global,
            verify,
            format,
            verbose,
        } => {
            run_status(scope, global, verify, format, verbose)?;
        }
        Commands::Install {
            kind,
            name,
            source,
            scope,
            force,
            runtime,
            transport,
            url,
            env,
            header,
            command,
        } => {
            run_install(InstallArgs {
                kind,
                name,
                source,
                scope,
                force,
                runtime,
                transport,
                url,
                env,
                headers: header,
                command,
            })?;
        }
        Commands::Uninstall { kind, name } => {
            println!("Uninstalling {kind}: {name}");
        }
        Commands::List { kind } => match kind.as_deref() {
            Some("mcp") => println!("Listing MCP servers"),
            Some("skill") => println!("Listing skills"),
            Some(_) | None => println!("Listing all"),
        },
        Commands::Config { scope } => {
            println!("Setting config scope to: {scope}");
        }
    }
    Ok(())
}

struct InstallArgs {
    kind: String,
    name: String,
    source: Option<String>,
    scope: Option<String>,
    force: bool,
    runtime: Option<String>,
    transport: Option<String>,
    url: Option<String>,
    env: Vec<String>,
    headers: Vec<String>,
    command: Vec<String>,
}

fn run_install(args: InstallArgs) -> Result<()> {
    // Parse target type
    let target = match args.kind.to_lowercase().as_str() {
        "mcp" => InstallTarget::Mcp,
        "skill" => InstallTarget::Skill,
        _ => anyhow::bail!("Unknown install type: {}. Use 'mcp' or 'skill'", args.kind),
    };

    // Parse scope if provided
    let config_scope = if let Some(s) = &args.scope {
        Some(parse_scope(s)?)
    } else {
        None
    };

    // Build options
    let (resolved_name, parsed_version) = split_name_and_version(&args.name)?;
    let mut options = match target {
        InstallTarget::Mcp => InstallOptions::mcp(resolved_name),
        InstallTarget::Skill => InstallOptions::skill(resolved_name),
    };

    if let Some(s) = &args.source {
        options = options.with_source(s);
    }
    if let Some(v) = parsed_version {
        options = options.with_version(v);
    }
    if let Some(s) = config_scope {
        options = options.with_scope(s);
    }
    if args.force {
        options = options.with_force(true);
    }
    if let Some(r) = &args.runtime {
        options = options.with_runtime(r);
    }
    if let Some(t) = &args.transport {
        options = options.with_transport(t);
    }
    if let Some(u) = &args.url {
        options = options.with_url(u);
    }
    for pair in &args.env {
        options = options.with_env(pair);
    }
    for pair in &args.headers {
        options = options.with_header(pair);
    }
    if !args.command.is_empty() {
        options = options.with_command(&args.command);
    }

    // Create and execute install command
    let cmd = InstallCommand::with_defaults()?;
    let report = cmd.execute(&options)?;

    // Print result
    if report.changed {
        println!("✓ Installed {} '{}'", args.kind, report.name);
    } else {
        println!("• {} '{}' is already installed", args.kind, report.name);
    }

    if report.applied {
        println!("  Applied to client configurations");
    }

    for warning in &report.warnings {
        println!("  ⚠ {}", warning);
    }

    Ok(())
}

fn split_name_and_version(input: &str) -> Result<(String, Option<String>)> {
    if is_local_path(input) || is_git_like(input) {
        return Ok((input.to_string(), None));
    }

    let Some(at_pos) = input.rfind('@') else {
        return Ok((input.to_string(), None));
    };
    if at_pos == 0 {
        return Ok((input.to_string(), None));
    }

    let (name, version) = input.split_at(at_pos);
    let version = version.trim_start_matches('@');
    if version.is_empty() {
        anyhow::bail!("Invalid version specifier: {}", input);
    }
    Ok((name.to_string(), Some(version.to_string())))
}

fn is_local_path(input: &str) -> bool {
    input.starts_with("./")
        || input.starts_with("../")
        || input.starts_with('/')
        || input.starts_with("~/")
}

fn is_git_like(input: &str) -> bool {
    input.starts_with("http://")
        || input.starts_with("https://")
        || input.starts_with("git+")
        || input.starts_with("github:")
        || input.starts_with("git:")
        || input.starts_with("git@")
}

fn run_status(
    scope: Option<String>,
    global: bool,
    verify: bool,
    format: OutputFormat,
    verbose: bool,
) -> Result<()> {
    // Determine scope filter
    let scope_filter = if global {
        Some(ConfigScope::Global)
    } else if let Some(s) = scope {
        Some(parse_scope(&s)?)
    } else {
        None
    };

    // Detect project root (current directory)
    let project_root = std::env::current_dir()?;

    // Collect status
    let status = collect_status(&project_root, scope_filter, verify)?;

    // Output based on format
    match format {
        OutputFormat::Table => print_table(&status, verbose),
        OutputFormat::Json => print_json(&status)?,
        OutputFormat::Quiet => {
            let exit_code = print_quiet(&status);
            if exit_code != 0 {
                std::process::exit(exit_code);
            }
        }
    }

    Ok(())
}

fn parse_scope(s: &str) -> Result<ConfigScope> {
    match s.to_lowercase().as_str() {
        "global" | "g" => Ok(ConfigScope::Global),
        "shared" | "project" | "p" => Ok(ConfigScope::PerProjectShared),
        "local" | "l" => Ok(ConfigScope::PerProjectLocal),
        _ => anyhow::bail!("Unknown scope: {}. Use 'global', 'shared', or 'local'", s),
    }
}

fn print_table(status: &SystemStatus, verbose: bool) {
    // Header
    if let Some(ref root) = status.project_root {
        println!("Project: {}", root.display());
    }

    let scope_str = match status.scope_filter {
        Some(ConfigScope::Global) => "global",
        Some(ConfigScope::PerProjectShared) => "shared (project)",
        Some(ConfigScope::PerProjectLocal) => "local",
        None => "merged (global + shared + local)",
    };
    println!("Scope: {}", scope_str);
    println!("Link Mode: {:?}", status.link_mode);
    println!();

    // MCP Servers
    if !status.mcp_servers.is_empty() {
        println!("MCP Servers ({}):", status.mcp_servers.len());
        if verbose {
            print_mcp_verbose(&status.mcp_servers);
        } else {
            print_mcp_table(&status.mcp_servers);
        }
        println!();
    }

    // Skills
    if !status.skills.is_empty() {
        println!("Skills ({}):", status.skills.len());
        if verbose {
            print_skills_verbose(&status.skills);
        } else {
            print_skills_table(&status.skills);
        }
        println!();
    }

    // Summary
    if status.mcp_servers.is_empty() && status.skills.is_empty() {
        println!("No MCP servers or skills configured.");
        println!("Create a sift.toml to get started.");
    } else {
        let total = status.summary.total_mcp + status.summary.total_skills;
        if status.summary.issues > 0 {
            println!(
                "Summary: {} entries, {} issues (run 'sift install <kind> <name>' to resolve)",
                total, status.summary.issues
            );
        } else {
            println!("Summary: {} entries, all OK", total);
        }
    }
}

fn print_mcp_table(servers: &[McpServerStatus]) {
    println!(
        "  {:<15} {:<10} {:<12} {:<10} {:<8} Status",
        "Name", "Runtime", "Version", "Constraint", "Scope"
    );
    println!("  {}", "-".repeat(70));

    for server in servers {
        let runtime = server.runtime.as_deref().unwrap_or("-");
        let version = server.resolved_version.as_deref().unwrap_or("-");
        let scope = scope_short(&server.scope);
        let status = state_symbol(&server.state);

        println!(
            "  {:<15} {:<10} {:<12} {:<10} {:<8} {}",
            truncate(&server.name, 15),
            runtime,
            truncate(version, 12),
            truncate(&server.constraint, 10),
            scope,
            status
        );
    }
}

fn print_mcp_verbose(servers: &[McpServerStatus]) {
    for server in servers {
        let version = server.resolved_version.as_deref().unwrap_or("-");
        let scope = scope_short(&server.scope);
        println!(
            "\n  {} ({} -> {}) [{}]",
            server.name, server.constraint, version, scope
        );

        for dep in &server.deployments {
            let symbol = deployment_symbol(&dep.integrity);
            println!(
                "    {} {} @ {}",
                symbol,
                dep.client_id,
                dep.config_path.display()
            );
        }

        if server.deployments.is_empty() {
            println!("    (no deployments)");
        }
    }
}

fn print_skills_table(skills: &[SkillStatus]) {
    println!(
        "  {:<15} {:<10} {:<12} {:<8} {:<20} Status",
        "Name", "Version", "Constraint", "Mode", "Path"
    );
    println!("  {}", "-".repeat(80));

    for skill in skills {
        let version = skill.resolved_version.as_deref().unwrap_or("-");
        let mode = skill
            .mode
            .map(|m| format!("{:?}", m))
            .unwrap_or_else(|| "-".to_string());
        let path = skill
            .dst_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".to_string());
        let status = state_symbol(&skill.state);

        println!(
            "  {:<15} {:<10} {:<12} {:<8} {:<20} {}",
            truncate(&skill.name, 15),
            truncate(version, 10),
            truncate(&skill.constraint, 12),
            mode,
            truncate(&path, 20),
            status
        );
    }
}

fn print_skills_verbose(skills: &[SkillStatus]) {
    for skill in skills {
        let version = skill.resolved_version.as_deref().unwrap_or("-");
        let scope = scope_short(&skill.scope);
        println!(
            "\n  {} ({} -> {}) [{}]",
            skill.name, skill.constraint, version, scope
        );

        for dep in &skill.deployments {
            let symbol = skill_integrity_symbol(&dep.integrity);
            println!(
                "    {} {} @ {} ({:?})",
                symbol,
                dep.client_id,
                dep.dst_path.display(),
                dep.mode
            );
        }

        if skill.deployments.is_empty() {
            if let Some(ref path) = skill.dst_path {
                println!("    Path: {}", path.display());
            } else {
                println!("    (not installed)");
            }
        }
    }
}

fn print_json(status: &SystemStatus) -> Result<()> {
    // Wrap in versioned output
    let output = serde_json::json!({
        "schema_version": 1,
        "project_root": status.project_root,
        "scope_filter": status.scope_filter,
        "link_mode": status.link_mode,
        "mcp_servers": status.mcp_servers,
        "skills": status.skills,
        "clients": status.clients,
        "summary": status.summary,
    });

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_quiet(status: &SystemStatus) -> i32 {
    if status.summary.issues > 0 {
        println!("{} issues found", status.summary.issues);
        1
    } else {
        0
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn state_symbol(state: &EntryState) -> &'static str {
    match state {
        EntryState::Ok => "OK",
        EntryState::NotLocked => "Not Locked",
        EntryState::Stale => "Stale",
        EntryState::Orphaned => "Orphaned",
    }
}

fn deployment_symbol(integrity: &sift_core::status::DeploymentIntegrity) -> &'static str {
    use sift_core::status::DeploymentIntegrity;
    match integrity {
        DeploymentIntegrity::Ok => "[OK]",
        DeploymentIntegrity::Modified => "[Modified]",
        DeploymentIntegrity::Missing => "[Missing]",
        DeploymentIntegrity::NotDeployed => "[Not Deployed]",
    }
}

fn skill_integrity_symbol(integrity: &sift_core::status::SkillIntegrity) -> &'static str {
    use sift_core::status::SkillIntegrity;
    match integrity {
        SkillIntegrity::Installed => "[OK]",
        SkillIntegrity::Modified => "[Modified]",
        SkillIntegrity::NotFound => "[Not Found]",
        SkillIntegrity::BrokenLink => "[Broken Link]",
        SkillIntegrity::NotDeployed => "[Not Deployed]",
    }
}

fn scope_short(scope: &ConfigScope) -> &'static str {
    match scope {
        ConfigScope::Global => "Global",
        ConfigScope::PerProjectShared => "Shared",
        ConfigScope::PerProjectLocal => "Local",
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

fn run_tui() -> Result<()> {
    println!("Launching TUI...");
    // The TUI will be implemented in sift-tui crate
    // For now, just stub it
    Ok(())
}

fn run_gui() {
    println!("Launching GUI...");
    // The GUI will be implemented in sift-gui crate
    // For now, just stub it
}

#[cfg(test)]
mod tests {
    use super::{Cli, split_name_and_version};
    use clap::Parser;

    #[test]
    fn parse_name_version_from_simple_name() {
        let (name, version) = split_name_and_version("demo@1.2.3").unwrap();
        assert_eq!(name, "demo");
        assert_eq!(version, Some("1.2.3".to_string()));
    }

    #[test]
    fn parse_name_version_from_scoped_name() {
        let (name, version) = split_name_and_version("@acme/tool@0.4.0").unwrap();
        assert_eq!(name, "@acme/tool");
        assert_eq!(version, Some("0.4.0".to_string()));
    }

    #[test]
    fn ignore_version_for_local_path() {
        let (name, version) = split_name_and_version("./skills/demo@1.0.0").unwrap();
        assert_eq!(name, "./skills/demo@1.0.0");
        assert_eq!(version, None);
    }

    #[test]
    fn ignore_version_for_git_url() {
        let (name, version) = split_name_and_version("git@github.com:acme/demo.git").unwrap();
        assert_eq!(name, "git@github.com:acme/demo.git");
        assert_eq!(version, None);
    }

    #[test]
    fn error_on_empty_version() {
        let result = split_name_and_version("demo@");
        assert!(result.is_err());
    }

    #[test]
    fn install_stdio_command_parses_without_panic() {
        let args = [
            "sift",
            "install",
            "mcp",
            "custom",
            "--transport",
            "stdio",
            "--",
            "npx",
            "-y",
            "@acme/server",
        ];

        let result = std::panic::catch_unwind(|| Cli::try_parse_from(args));
        assert!(result.is_ok(), "CLI parsing should not panic");
        assert!(result.unwrap().is_ok(), "CLI parsing should succeed");
    }

    #[test]
    fn install_http_url_parses_without_panic() {
        let args = [
            "sift",
            "install",
            "mcp",
            "custom",
            "--transport",
            "http",
            "--url",
            "https://mcp.example.com",
        ];

        let result = std::panic::catch_unwind(|| Cli::try_parse_from(args));
        assert!(result.is_ok(), "CLI parsing should not panic");
        assert!(result.unwrap().is_ok(), "CLI parsing should succeed");
    }
}
