//! Interactive flow for install command.
//!
//! Collects install options interactively when `-i` flag is passed.
//! Uses dialoguer for terminal UI prompts.

use std::collections::HashMap;
use std::io::{self, Write};

use anyhow::Result;
use console::style;
use dialoguer::{Confirm, Input, MultiSelect, Select, theme::ColorfulTheme};

use sift_core::client::registry::ClientRegistry;
use sift_core::commands::{InstallOptions, InstallTarget};
use sift_core::registry::RegistryConfig;
use sift_core::types::ConfigScope;

/// Pre-filled values from CLI args that skip prompts.
#[derive(Debug, Clone, Default)]
pub struct PrefilledOptions {
    /// Target type (mcp or skill) - if Some, skip kind prompt
    pub kind: Option<InstallTarget>,
    /// Package name - if Some, skip name prompt
    pub name: Option<String>,
    /// Registry name - if Some, skip registry selection
    pub registry: Option<String>,
    /// Configuration scope - if Some, skip scope prompt
    pub scope: Option<ConfigScope>,
    /// Target clients - if Some, skip client selection
    pub targets: Option<Vec<String>>,
    /// Force flag
    pub force: bool,
    /// Skip all confirmations
    pub yes: bool,
}

/// Result of interactive flow.
#[derive(Debug, Clone)]
pub struct InteractiveResult {
    /// The populated install options
    pub options: InstallOptions,
    /// Whether user confirmed the install
    pub confirmed: bool,
}

/// Interactive flow for collecting install options.
///
/// Prompts user for missing options and returns a fully populated InstallOptions.
pub struct InteractiveFlow<W: Write = io::Stdout> {
    /// Available registries
    registries: HashMap<String, RegistryConfig>,
    /// Client registry for target selection
    client_registry: ClientRegistry,
    /// Pre-filled options from CLI args
    prefilled: PrefilledOptions,
    /// Output writer (for testing)
    writer: W,
    /// Theme for dialoguer prompts
    theme: ColorfulTheme,
}

impl InteractiveFlow<io::Stdout> {
    /// Create a new interactive flow with stdout.
    pub fn new(
        registries: HashMap<String, RegistryConfig>,
        client_registry: ClientRegistry,
        prefilled: PrefilledOptions,
    ) -> Self {
        Self {
            registries,
            client_registry,
            prefilled,
            writer: io::stdout(),
            theme: ColorfulTheme::default(),
        }
    }
}

impl<W: Write> InteractiveFlow<W> {
    /// Create a new interactive flow with custom writer (for testing).
    #[cfg(test)]
    pub fn with_writer(
        registries: HashMap<String, RegistryConfig>,
        client_registry: ClientRegistry,
        prefilled: PrefilledOptions,
        writer: W,
    ) -> Self {
        Self {
            registries,
            client_registry,
            prefilled,
            writer,
            theme: ColorfulTheme::default(),
        }
    }

    /// Run the interactive flow and collect install options.
    ///
    /// Flow:
    /// 1. Select target type (mcp or skill) if not provided
    /// 2. Select registry if multiple configured
    /// 3. Input package name if not provided
    /// 4. Select scope (global/shared/local) if not provided
    /// 5. Select target clients if not provided
    /// 6. Show summary and confirm
    pub fn collect(&mut self) -> Result<InteractiveResult> {
        self.print_header()?;

        // Step 1: Target type
        let target = self.prompt_target_type()?;

        // Step 2: Registry selection (if multiple)
        let registry = self.prompt_registry()?;

        // Step 3: Package name
        let name = self.prompt_name()?;

        // Step 4: Scope
        let scope = self.prompt_scope()?;

        // Step 5: Target clients
        let targets = self.prompt_targets(&scope, target)?;

        // Build options
        let mut options = match target {
            InstallTarget::Mcp => InstallOptions::mcp(&name),
            InstallTarget::Skill => InstallOptions::skill(&name),
        };

        if let Some(reg) = &registry {
            options = options.with_registry(reg);
        }
        options = options.with_scope(scope);
        if self.prefilled.force {
            options = options.with_force(true);
        }
        if !targets.is_empty() {
            options = options.with_targets(&targets);
        }

        // Step 6: Summary and confirm
        let confirmed = self.show_summary_and_confirm(&options, &registry)?;

        Ok(InteractiveResult { options, confirmed })
    }

    fn print_header(&mut self) -> Result<()> {
        writeln!(self.writer)?;
        writeln!(
            self.writer,
            "{}",
            style("  Sift Install Wizard").bold().cyan()
        )?;
        writeln!(self.writer)?;
        Ok(())
    }

    fn prompt_target_type(&self) -> Result<InstallTarget> {
        if let Some(kind) = self.prefilled.kind {
            return Ok(kind);
        }

        let options = vec!["MCP Server", "Skill"];
        let selection = Select::with_theme(&self.theme)
            .with_prompt("What do you want to install?")
            .items(&options)
            .default(0)
            .interact()?;

        Ok(match selection {
            0 => InstallTarget::Mcp,
            _ => InstallTarget::Skill,
        })
    }

    fn prompt_registry(&self) -> Result<Option<String>> {
        if let Some(reg) = &self.prefilled.registry {
            return Ok(Some(reg.clone()));
        }

        let registry_names: Vec<_> = self.registries.keys().cloned().collect();

        match registry_names.len() {
            0 => Ok(None),
            1 => Ok(Some(registry_names[0].clone())),
            _ => {
                let selection = Select::with_theme(&self.theme)
                    .with_prompt("Select registry")
                    .items(&registry_names)
                    .default(0)
                    .interact()?;

                Ok(Some(registry_names[selection].clone()))
            }
        }
    }

    fn prompt_name(&self) -> Result<String> {
        if let Some(name) = &self.prefilled.name {
            return Ok(name.clone());
        }

        let name: String = Input::with_theme(&self.theme)
            .with_prompt("Package name")
            .interact_text()?;

        Ok(name)
    }

    fn prompt_scope(&self) -> Result<ConfigScope> {
        if let Some(scope) = self.prefilled.scope {
            return Ok(scope);
        }

        let options = vec![
            "Global   - User-wide, personal tools",
            "Shared   - Project-wide, committed to git",
            "Local    - Project-local, private overrides",
        ];

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Configuration scope")
            .items(&options)
            .default(1)
            .interact()?;

        Ok(match selection {
            0 => ConfigScope::Global,
            1 => ConfigScope::PerProjectShared,
            _ => ConfigScope::PerProjectLocal,
        })
    }

    fn prompt_targets(&self, scope: &ConfigScope, target: InstallTarget) -> Result<Vec<String>> {
        if let Some(targets) = &self.prefilled.targets {
            return Ok(targets.clone());
        }

        let available_clients: Vec<_> = match target {
            InstallTarget::Mcp => self.client_registry.mcp_clients_for_scope(*scope),
            InstallTarget::Skill => self.client_registry.skill_clients_for_scope(*scope),
        };

        if available_clients.is_empty() {
            return Ok(vec![]);
        }

        let client_names: Vec<_> = available_clients.iter().map(|c| c.id()).collect();

        let defaults: Vec<bool> = client_names.iter().map(|_| true).collect();

        let selections = MultiSelect::with_theme(&self.theme)
            .with_prompt("Target clients (space to toggle, enter to confirm)")
            .items(&client_names)
            .defaults(&defaults)
            .interact()?;

        if selections.len() == client_names.len() {
            // All selected = no filter
            Ok(vec![])
        } else {
            Ok(selections
                .iter()
                .map(|&i| client_names[i].to_string())
                .collect())
        }
    }

    fn show_summary_and_confirm(
        &mut self,
        options: &InstallOptions,
        registry: &Option<String>,
    ) -> Result<bool> {
        writeln!(self.writer)?;
        writeln!(self.writer, "{}", style("  Summary").bold())?;
        writeln!(self.writer, "  ───────────────────────────")?;

        let kind = match options.target {
            InstallTarget::Mcp => "MCP Server",
            InstallTarget::Skill => "Skill",
        };
        writeln!(self.writer, "  Type:     {}", style(kind).green())?;
        writeln!(self.writer, "  Name:     {}", style(&options.name).green())?;

        if let Some(reg) = registry {
            writeln!(self.writer, "  Registry: {}", style(reg).green())?;
        }

        let scope_str = match options.scope.unwrap_or(ConfigScope::PerProjectShared) {
            ConfigScope::Global => "Global",
            ConfigScope::PerProjectShared => "Shared",
            ConfigScope::PerProjectLocal => "Local",
        };
        writeln!(self.writer, "  Scope:    {}", style(scope_str).green())?;

        if let Some(targets) = &options.targets
            && !targets.is_empty()
        {
            writeln!(
                self.writer,
                "  Targets:  {}",
                style(targets.join(", ")).green()
            )?;
        }

        writeln!(self.writer)?;

        if self.prefilled.yes {
            return Ok(true);
        }

        let confirmed = Confirm::with_theme(&self.theme)
            .with_prompt("Proceed with installation?")
            .default(true)
            .interact()?;

        Ok(confirmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_registries() -> HashMap<String, RegistryConfig> {
        let mut registries = HashMap::new();
        registries.insert(
            "official".to_string(),
            RegistryConfig {
                r#type: sift_core::registry::RegistryType::ClaudeMarketplace,
                url: None,
                source: Some("github:anthropics/skills".to_string()),
            },
        );
        registries
    }

    fn make_multi_registries() -> HashMap<String, RegistryConfig> {
        let mut registries = make_registries();
        registries.insert(
            "community".to_string(),
            RegistryConfig {
                r#type: sift_core::registry::RegistryType::ClaudeMarketplace,
                url: None,
                source: Some("github:community/skills".to_string()),
            },
        );
        registries
    }

    #[test]
    fn test_prefilled_options_default() {
        let prefilled = PrefilledOptions::default();

        assert!(prefilled.kind.is_none());
        assert!(prefilled.name.is_none());
        assert!(prefilled.registry.is_none());
        assert!(prefilled.scope.is_none());
        assert!(prefilled.targets.is_none());
        assert!(!prefilled.force);
        assert!(!prefilled.yes);
    }

    #[test]
    fn test_prefilled_skips_prompts() {
        let prefilled = PrefilledOptions {
            kind: Some(InstallTarget::Skill),
            name: Some("demo-skill".to_string()),
            registry: Some("official".to_string()),
            scope: Some(ConfigScope::Global),
            targets: Some(vec!["claude-code".to_string()]),
            force: true,
            yes: true,
        };

        let mut output = Vec::new();
        let mut flow = InteractiveFlow::with_writer(
            make_registries(),
            ClientRegistry::with_default_clients(),
            prefilled,
            &mut output,
        );

        let result = flow.collect().unwrap();

        assert!(result.confirmed);
        assert_eq!(result.options.target, InstallTarget::Skill);
        assert_eq!(result.options.name, "demo-skill");
        assert_eq!(result.options.scope, Some(ConfigScope::Global));
        assert!(result.options.force);
        assert_eq!(
            result.options.targets,
            Some(vec!["claude-code".to_string()])
        );
    }

    #[test]
    fn test_single_registry_auto_selects() {
        let prefilled = PrefilledOptions {
            kind: Some(InstallTarget::Mcp),
            name: Some("test-mcp".to_string()),
            scope: Some(ConfigScope::PerProjectShared),
            targets: Some(vec![]),
            yes: true,
            ..Default::default()
        };

        let mut output = Vec::new();
        let mut flow = InteractiveFlow::with_writer(
            make_registries(),
            ClientRegistry::with_default_clients(),
            prefilled,
            &mut output,
        );

        let result = flow.collect().unwrap();

        assert!(result.confirmed);
        assert_eq!(result.options.name, "test-mcp");
    }

    #[test]
    fn test_no_registries_returns_none() {
        let prefilled = PrefilledOptions {
            kind: Some(InstallTarget::Skill),
            name: Some("test".to_string()),
            scope: Some(ConfigScope::Global),
            targets: Some(vec![]),
            yes: true,
            ..Default::default()
        };

        let mut output = Vec::new();
        let mut flow = InteractiveFlow::with_writer(
            HashMap::new(),
            ClientRegistry::with_default_clients(),
            prefilled,
            &mut output,
        );

        let result = flow.collect().unwrap();

        assert!(result.confirmed);
        assert!(result.options.registry.is_none());
    }

    #[test]
    fn test_yes_flag_skips_confirmation() {
        let prefilled = PrefilledOptions {
            kind: Some(InstallTarget::Mcp),
            name: Some("test".to_string()),
            scope: Some(ConfigScope::Global),
            targets: Some(vec![]),
            yes: true,
            ..Default::default()
        };

        let mut output = Vec::new();
        let mut flow = InteractiveFlow::with_writer(
            make_registries(),
            ClientRegistry::with_default_clients(),
            prefilled,
            &mut output,
        );

        let result = flow.collect().unwrap();

        assert!(result.confirmed);
    }

    #[test]
    fn test_force_flag_propagates() {
        let prefilled = PrefilledOptions {
            kind: Some(InstallTarget::Skill),
            name: Some("test".to_string()),
            scope: Some(ConfigScope::PerProjectShared),
            targets: Some(vec![]),
            force: true,
            yes: true,
            ..Default::default()
        };

        let mut output = Vec::new();
        let mut flow = InteractiveFlow::with_writer(
            make_registries(),
            ClientRegistry::with_default_clients(),
            prefilled,
            &mut output,
        );

        let result = flow.collect().unwrap();

        assert!(result.options.force);
    }

    #[test]
    fn test_summary_output_format() {
        let prefilled = PrefilledOptions {
            kind: Some(InstallTarget::Skill),
            name: Some("demo-skill".to_string()),
            registry: Some("official".to_string()),
            scope: Some(ConfigScope::Global),
            targets: Some(vec!["claude-code".to_string()]),
            yes: true,
            ..Default::default()
        };

        let mut output = Vec::new();
        let mut flow = InteractiveFlow::with_writer(
            make_registries(),
            ClientRegistry::with_default_clients(),
            prefilled,
            &mut output,
        );

        flow.collect().unwrap();

        let output_str = String::from_utf8(output).unwrap();
        assert!(output_str.contains("Summary"));
        assert!(output_str.contains("Skill"));
        assert!(output_str.contains("demo-skill"));
        assert!(output_str.contains("Global"));
    }

    #[test]
    fn test_empty_targets_means_all_clients() {
        let prefilled = PrefilledOptions {
            kind: Some(InstallTarget::Mcp),
            name: Some("test".to_string()),
            scope: Some(ConfigScope::PerProjectShared),
            targets: Some(vec![]),
            yes: true,
            ..Default::default()
        };

        let mut output = Vec::new();
        let mut flow = InteractiveFlow::with_writer(
            make_registries(),
            ClientRegistry::with_default_clients(),
            prefilled,
            &mut output,
        );

        let result = flow.collect().unwrap();

        // Empty targets = no filter = all clients
        assert!(
            result.options.targets.is_none()
                || result.options.targets.as_ref().map(|t| t.is_empty()) == Some(true)
        );
    }

    #[test]
    fn test_multiple_registries_requires_selection() {
        let prefilled = PrefilledOptions {
            kind: Some(InstallTarget::Skill),
            name: Some("test".to_string()),
            registry: Some("community".to_string()),
            scope: Some(ConfigScope::Global),
            targets: Some(vec![]),
            yes: true,
            ..Default::default()
        };

        let mut output = Vec::new();
        let mut flow = InteractiveFlow::with_writer(
            make_multi_registries(),
            ClientRegistry::with_default_clients(),
            prefilled,
            &mut output,
        );

        let result = flow.collect().unwrap();

        assert!(result.confirmed);
        assert_eq!(result.options.registry, Some("community".to_string()));
    }
}
