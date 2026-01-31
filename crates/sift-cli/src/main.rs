//! Sift - MCP & Skills Manager
//!
//! Usage:
//!   sift              # Launch TUI (default)
//!   sift status       # Show installation status
//!   sift install ...  # CLI operations
//!   sift --gui        # Launch GUI

mod interactive;

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use sift_core::commands::context::InstallContext;
use sift_core::commands::{
    InstallCommand, InstallOptions, InstallTarget, StatusCommand, StatusOptions, UninstallCommand,
    UninstallOptions,
};
use sift_core::commands::{
    RegistryAddOptions, RegistryCommand, RegistryEntry, RegistryListOptions, RegistryRemoveOptions,
};
use sift_core::registry::RegistryType;
use sift_core::status::{EntryState, McpServerStatus, SkillStatus, SystemStatus};
use sift_core::types::ConfigScope;

use crate::interactive::{InteractiveFlow, PrefilledOptions};

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
    Install(Box<InstallArgs>),

    /// Uninstall an MCP server or skill
    ///
    /// By default (no --scope), Sift auto-detects where the package is installed:
    /// 1. Checks lockfile for recorded scope
    /// 2. Falls back to searching all configs
    #[command(alias = "rm")]
    Uninstall {
        /// What to uninstall (mcp or skill)
        kind: String,
        /// Name/ID of the package to uninstall
        name: String,
        /// Configuration scope
        ///
        /// - auto (default): Detect from lockfile, then search configs
        /// - global: Remove from ~/.config/sift/sift.toml
        /// - shared: Remove from ./sift.toml (project-wide, committed to git)
        /// - local: Remove from project-local override in ~/.config/sift/sift.toml
        /// - all: Remove from all scopes where installed
        #[arg(long)]
        scope: Option<String>,
        /// Output format
        #[arg(short, long, default_value = "table")]
        format: OutputFormat,
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

    /// Manage registries
    Registry(RegistryArgs),
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

#[derive(Args)]
struct RegistryArgs {
    #[command(subcommand)]
    command: RegistrySubcommand,
}

#[derive(Subcommand)]
enum RegistrySubcommand {
    /// List configured registries
    List {
        /// Filter by scope (global, shared)
        #[arg(long)]
        scope: Option<String>,

        /// Output format
        #[arg(short, long, default_value = "table")]
        format: OutputFormat,
    },

    /// Add a new registry
    Add {
        /// Registry name (identifier used in sift.toml)
        name: String,

        /// Registry URL or source (https:// for sift, github:org/repo for marketplace)
        source: String,

        /// Registry type (auto-detected if not specified)
        #[arg(long, short = 't', value_parser = ["sift", "claude-marketplace"])]
        r#type: Option<String>,

        /// Configuration scope
        #[arg(long, default_value = "global")]
        scope: String,

        /// Overwrite existing registry with same name
        #[arg(long, short)]
        force: bool,

        /// Output format
        #[arg(short = 'o', long, default_value = "table")]
        format: OutputFormat,
    },

    /// Remove a registry
    Remove {
        /// Registry name to remove
        name: String,

        /// Configuration scope to remove from
        #[arg(long)]
        scope: Option<String>,

        /// Remove from all scopes
        #[arg(long)]
        all: bool,

        /// Output format
        #[arg(short, long, default_value = "table")]
        format: OutputFormat,
    },
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
        Commands::Install(args) => {
            run_install(*args)?;
        }
        Commands::Uninstall {
            kind,
            name,
            scope,
            format,
        } => {
            run_uninstall(kind, name, scope, format)?;
        }
        Commands::List { kind } => match kind.as_deref() {
            Some("mcp") => println!("Listing MCP servers"),
            Some("skill") => println!("Listing skills"),
            Some(_) | None => println!("Listing all"),
        },
        Commands::Config { scope } => {
            println!("Setting config scope to: {scope}");
        }
        Commands::Registry(args) => {
            run_registry(args)?;
        }
    }
    Ok(())
}

#[derive(Args)]
struct InstallArgs {
    /// What to install (mcp or skill)
    ///
    /// Required unless --interactive is used
    kind: Option<String>,
    /// Name/ID of the package to install
    ///
    /// Required unless --interactive is used
    name: Option<String>,
    /// Interactive mode - prompts for missing options
    #[arg(short, long)]
    interactive: bool,
    /// Skip all confirmation prompts (for CI/CD)
    #[arg(short = 'y', long)]
    yes: bool,
    /// Source specification (e.g., "registry:name" or "local:/path")
    #[arg(long, short)]
    source: Option<String>,
    /// Registry name to disambiguate when multiple registries exist
    #[arg(long)]
    registry: Option<String>,
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
    #[arg(long = "header", value_name = "KEY=VALUE")]
    headers: Vec<String>,
    /// Stdio command for MCP servers (after --)
    #[arg(last = true)]
    command: Vec<String>,
    /// Target clients (whitelist) - only deploy to these clients
    #[arg(long = "target", value_name = "CLIENT")]
    targets: Vec<String>,
    /// Ignore clients (blacklist) - deploy to all except these
    #[arg(long = "ignore-target", value_name = "CLIENT")]
    ignore_targets: Vec<String>,
    /// Output format
    #[arg(short = 'o', long, default_value = "table")]
    format: OutputFormat,
}

fn run_install(args: InstallArgs) -> Result<()> {
    // Handle interactive mode
    if args.interactive {
        return run_install_interactive(args);
    }

    // Non-interactive mode requires kind and name
    let kind = args
        .kind
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing required argument: kind (mcp or skill)"))?;
    let name = args
        .name
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing required argument: name"))?;

    // Parse target type
    let target = match kind.to_lowercase().as_str() {
        "mcp" => InstallTarget::Mcp,
        "skill" => InstallTarget::Skill,
        _ => anyhow::bail!("Unknown install type: {}. Use 'mcp' or 'skill'", kind),
    };

    // Parse scope if provided
    let config_scope = if let Some(s) = &args.scope {
        Some(parse_scope(s)?)
    } else {
        None
    };

    // Build options
    let (resolved_name, parsed_version) = split_name_and_version(name)?;
    let mut options = match target {
        InstallTarget::Mcp => InstallOptions::mcp(resolved_name),
        InstallTarget::Skill => InstallOptions::skill(resolved_name),
    };

    if let Some(s) = &args.source {
        options = options.with_source(s);
    }
    if let Some(registry) = &args.registry {
        options = options.with_registry(registry);
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
    if !args.targets.is_empty() {
        options = options.with_targets(&args.targets);
    }
    if !args.ignore_targets.is_empty() {
        options = options.with_ignore_targets(&args.ignore_targets);
    }

    // Create and execute install command
    let cmd = InstallCommand::with_defaults()?;
    let report = cmd.execute(&options)?;

    // Print result
    print_install_result(&args, &report)?;

    Ok(())
}

fn run_install_interactive(args: InstallArgs) -> Result<()> {
    // Load context to get registries and client registry
    let ctx = InstallContext::with_defaults()?;
    let registries = ctx.registries()?;
    let client_registry = ctx.client_registry();

    // Parse pre-filled options from CLI args
    let kind = args
        .kind
        .as_ref()
        .and_then(|k| match k.to_lowercase().as_str() {
            "mcp" => Some(InstallTarget::Mcp),
            "skill" => Some(InstallTarget::Skill),
            _ => None,
        });

    let scope = args.scope.as_ref().and_then(|s| parse_scope(s).ok());

    let targets = if !args.targets.is_empty() {
        Some(args.targets.clone())
    } else {
        None
    };

    let prefilled = PrefilledOptions {
        kind,
        name: args.name.clone(),
        registry: args.registry.clone(),
        scope,
        targets,
        force: args.force,
        yes: args.yes,
    };

    // Run interactive flow
    let mut flow = InteractiveFlow::new(registries, client_registry, prefilled);
    let result = flow.collect()?;

    if !result.confirmed {
        println!("Installation cancelled.");
        return Ok(());
    }

    // Apply additional options that aren't collected interactively
    let mut options = result.options;

    if let Some(s) = &args.source {
        options = options.with_source(s);
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
    if !args.ignore_targets.is_empty() {
        options = options.with_ignore_targets(&args.ignore_targets);
    }

    // Execute install
    let cmd = InstallCommand::with_defaults()?;
    let report = cmd.execute(&options)?;

    // Print result
    print_install_result(&args, &report)?;

    Ok(())
}

fn print_install_result(
    args: &InstallArgs,
    report: &sift_core::commands::InstallReport,
) -> Result<()> {
    let kind = args.kind.as_deref().unwrap_or("package");

    match args.format {
        OutputFormat::Table => {
            if report.changed {
                println!("✓ Installed {} '{}'", kind, report.name);
            } else {
                println!("• {} '{}' is already installed", kind, report.name);
            }

            if report.applied {
                println!("  Applied to client configurations");
            }

            for warning in &report.warnings {
                println!("  ⚠ {}", warning);
            }
        }
        OutputFormat::Json => {
            let output = serde_json::json!({
                "name": report.name,
                "kind": kind,
                "changed": report.changed,
                "applied": report.applied,
                "warnings": report.warnings,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Quiet => {}
    }

    Ok(())
}

fn run_uninstall(
    kind: String,
    name: String,
    scope: Option<String>,
    format: OutputFormat,
) -> Result<()> {
    let target = match kind.to_lowercase().as_str() {
        "mcp" => UninstallOptions::mcp(name),
        "skill" => UninstallOptions::skill(name),
        _ => anyhow::bail!("Unknown uninstall type: {}. Use 'mcp' or 'skill'", kind),
    };

    let mut options = target;
    if let Some(scope) = scope {
        if scope.to_lowercase() == "all" {
            options = options.with_scope_all();
        } else {
            options = options.with_scope(parse_scope(&scope)?);
        }
    }

    let cmd = UninstallCommand::with_defaults()?;
    let report = cmd.execute(&options)?;

    match format {
        OutputFormat::Table => {
            println!("✓ Uninstalled {} '{}'", kind, report.name);
            for warning in &report.warnings {
                println!("  ⚠ {}", warning);
            }
        }
        OutputFormat::Json => {
            let output = serde_json::json!({
                "name": report.name,
                "changed": report.changed,
                "warnings": report.warnings,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Quiet => {}
    }

    Ok(())
}

fn run_registry(args: RegistryArgs) -> Result<()> {
    let cmd = RegistryCommand::with_defaults()?;

    match args.command {
        RegistrySubcommand::List { scope, format } => {
            let scope_filter = scope.as_deref().map(parse_registry_scope).transpose()?;
            let options = if let Some(s) = scope_filter {
                RegistryListOptions::new().with_scope(s)
            } else {
                RegistryListOptions::new()
            };

            let entries = cmd.list(&options)?;

            match format {
                OutputFormat::Table => print_registry_table(&entries),
                OutputFormat::Json => print_registry_json(&entries)?,
                OutputFormat::Quiet => {
                    // Just count - no output unless there are issues
                    if entries.is_empty() {
                        println!("No registries configured");
                    }
                }
            }
        }
        RegistrySubcommand::Add {
            name,
            source,
            r#type,
            scope,
            force,
            format,
        } => {
            let config_scope = parse_registry_scope(&scope)?;
            let registry_type = r#type.as_deref().map(parse_registry_type).transpose()?;

            let mut options = RegistryAddOptions::new(&name, &source)
                .with_scope(config_scope)
                .with_force(force);

            if let Some(t) = registry_type {
                options = options.with_type(t);
            }

            let report = cmd.add(&options)?;

            match format {
                OutputFormat::Table => {
                    if report.changed {
                        println!(
                            "Added registry '{}' ({:?} scope)",
                            report.name, report.scope
                        );
                    } else {
                        println!("Registry '{}' is already configured", report.name);
                    }

                    for warning in &report.warnings {
                        println!("  Warning: {}", warning);
                    }
                }
                OutputFormat::Json => {
                    let output = serde_json::json!({
                        "name": report.name,
                        "scope": format!("{:?}", report.scope),
                        "changed": report.changed,
                        "warnings": report.warnings,
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
                OutputFormat::Quiet => {}
            }
        }
        RegistrySubcommand::Remove {
            name,
            scope,
            all,
            format,
        } => {
            let scope_filter = scope.as_deref().map(parse_registry_scope).transpose()?;

            let mut options = RegistryRemoveOptions::new(&name).with_all_scopes(all);

            if let Some(s) = scope_filter {
                options = options.with_scope(s);
            }

            let report = cmd.remove(&options)?;

            match format {
                OutputFormat::Table => {
                    println!(
                        "Removed registry '{}' from {:?} scope",
                        report.name, report.scope
                    );
                }
                OutputFormat::Json => {
                    let output = serde_json::json!({
                        "name": report.name,
                        "scope": format!("{:?}", report.scope),
                        "removed": true,
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
                OutputFormat::Quiet => {}
            }
        }
    }

    Ok(())
}

fn parse_registry_scope(s: &str) -> Result<ConfigScope> {
    match s.to_lowercase().as_str() {
        "global" | "g" => Ok(ConfigScope::Global),
        "shared" | "project" | "p" => Ok(ConfigScope::PerProjectShared),
        _ => anyhow::bail!(
            "Invalid scope for registry: '{}'. Use 'global' or 'shared'",
            s
        ),
    }
}

fn parse_registry_type(s: &str) -> Result<RegistryType> {
    match s.to_lowercase().as_str() {
        "sift" => Ok(RegistryType::Sift),
        "claude-marketplace" => Ok(RegistryType::ClaudeMarketplace),
        _ => anyhow::bail!(
            "Invalid registry type: '{}'. Use 'sift' or 'claude-marketplace'",
            s
        ),
    }
}

fn print_registry_table(entries: &[RegistryEntry]) {
    if entries.is_empty() {
        println!("No registries configured.");
        println!("Add one with: sift registry add <name> <source>");
        return;
    }

    println!("{:<20} {:<20} {:<10} Source", "Name", "Type", "Scope");
    println!("{}", "-".repeat(70));

    for entry in entries {
        let type_str = match entry.registry_type {
            RegistryType::Sift => "sift",
            RegistryType::ClaudeMarketplace => "claude-marketplace",
        };
        let scope_str = match entry.scope {
            ConfigScope::Global => "global",
            ConfigScope::PerProjectShared => "shared",
            ConfigScope::PerProjectLocal => "local",
        };
        let source = entry
            .url
            .as_deref()
            .or(entry.source.as_deref())
            .unwrap_or("-");

        println!(
            "{:<20} {:<20} {:<10} {}",
            entry.name, type_str, scope_str, source
        );
    }
}

fn print_registry_json(entries: &[RegistryEntry]) -> Result<()> {
    let output: Vec<_> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "name": e.name,
                "type": match e.registry_type {
                    RegistryType::Sift => "sift",
                    RegistryType::ClaudeMarketplace => "claude-marketplace",
                },
                "scope": match e.scope {
                    ConfigScope::Global => "global",
                    ConfigScope::PerProjectShared => "shared",
                    ConfigScope::PerProjectLocal => "local",
                },
                "url": e.url,
                "source": e.source,
            })
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&output)?);
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

    // Build status options
    let mut options = StatusOptions::new().with_verify(verify);
    if let Some(scope) = scope_filter {
        options = options.with_scope(scope);
    }

    // Collect status using the command pattern
    let cmd = StatusCommand::with_defaults()?;
    let report = cmd.execute(&options)?;

    // Output based on format
    match format {
        OutputFormat::Table => print_table(&report, verbose),
        OutputFormat::Json => print_json(&report)?,
        OutputFormat::Quiet => {
            let exit_code = print_quiet(&report);
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

    #[test]
    fn install_with_registry_parses_without_panic() {
        let args = [
            "sift",
            "install",
            "skill",
            "demo-skill",
            "--registry",
            "official",
        ];

        let result = std::panic::catch_unwind(|| Cli::try_parse_from(args));
        assert!(result.is_ok(), "CLI parsing should not panic");
        assert!(result.unwrap().is_ok(), "CLI parsing should succeed");
    }

    #[test]
    fn registry_list_parses_without_panic() {
        let args = ["sift", "registry", "list"];

        let result = std::panic::catch_unwind(|| Cli::try_parse_from(args));
        assert!(result.is_ok(), "CLI parsing should not panic");
        assert!(result.unwrap().is_ok(), "CLI parsing should succeed");
    }

    #[test]
    fn registry_list_with_scope_parses() {
        let args = ["sift", "registry", "list", "--scope", "global"];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn registry_add_parses_without_panic() {
        let args = [
            "sift",
            "registry",
            "add",
            "official",
            "https://registry.sift.sh/v1",
        ];

        let result = std::panic::catch_unwind(|| Cli::try_parse_from(args));
        assert!(result.is_ok(), "CLI parsing should not panic");
        assert!(result.unwrap().is_ok(), "CLI parsing should succeed");
    }

    #[test]
    fn registry_add_with_type_parses() {
        let args = [
            "sift",
            "registry",
            "add",
            "anthropic",
            "github:anthropics/skills",
            "--type",
            "claude-marketplace",
        ];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn registry_add_with_scope_and_force_parses() {
        let args = [
            "sift",
            "registry",
            "add",
            "test-reg",
            "https://example.com/v1",
            "--scope",
            "shared",
            "--force",
        ];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn registry_remove_parses_without_panic() {
        let args = ["sift", "registry", "remove", "official"];

        let result = std::panic::catch_unwind(|| Cli::try_parse_from(args));
        assert!(result.is_ok(), "CLI parsing should not panic");
        assert!(result.unwrap().is_ok(), "CLI parsing should succeed");
    }

    #[test]
    fn registry_remove_with_all_flag_parses() {
        let args = ["sift", "registry", "remove", "official", "--all"];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn install_with_format_json_parses() {
        let args = ["sift", "install", "mcp", "test-server", "--format", "json"];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn install_with_format_short_option_parses() {
        let args = ["sift", "install", "mcp", "test-server", "-o", "json"];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn uninstall_with_format_json_parses() {
        let args = [
            "sift",
            "uninstall",
            "mcp",
            "test-server",
            "--format",
            "json",
        ];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn status_with_format_json_parses() {
        let args = ["sift", "status", "--format", "json"];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn registry_list_with_format_json_parses() {
        let args = ["sift", "registry", "list", "--format", "json"];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn registry_add_with_format_json_parses() {
        let args = [
            "sift",
            "registry",
            "add",
            "test-reg",
            "https://example.com/v1",
            "--format",
            "json",
        ];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn registry_remove_with_format_json_parses() {
        let args = ["sift", "registry", "remove", "test-reg", "--format", "json"];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn install_interactive_flag_parses() {
        let args = ["sift", "install", "-i"];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn install_interactive_with_partial_args_parses() {
        let args = [
            "sift",
            "install",
            "--interactive",
            "--scope",
            "global",
            "--registry",
            "official",
        ];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn install_interactive_with_kind_parses() {
        let args = ["sift", "install", "skill", "-i"];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn install_yes_flag_parses() {
        let args = ["sift", "install", "mcp", "test-mcp", "-y"];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn install_interactive_and_yes_flags_parse() {
        let args = ["sift", "install", "-i", "-y"];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn install_target_clients_flag_parses() {
        let args = [
            "sift",
            "install",
            "mcp",
            "test-mcp",
            "--target",
            "claude-code",
            "--target",
            "vscode",
        ];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }

    #[test]
    fn install_ignore_target_flag_parses() {
        let args = [
            "sift",
            "install",
            "skill",
            "test-skill",
            "--ignore-target",
            "codex",
        ];

        let cli = Cli::try_parse_from(args).unwrap();
        assert!(cli.command.is_some());
    }
}
