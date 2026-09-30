//! Configuration structures for herdr-auto-layout.
//!
//! Parses a YAML config file that declares layouts (tabs + panes) and optional
//! workspace-to-layout mappings.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Root configuration.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Fallback layout id applied when no workspace-specific match is found.
    pub global_layout: Option<String>,

    /// Available layout definitions.
    #[serde(default)]
    pub layouts: Vec<Layout>,

    /// Optional workspace-to-layout mappings.
    #[serde(default)]
    pub workspaces: Vec<WorkspaceMatch>,
}

/// A named layout containing one or more tabs.
#[derive(Debug, Deserialize, Serialize)]
pub struct Layout {
    /// Unique identifier referenced by `globalLayout` or workspace mappings.
    pub id: String,

    /// Tabs to create inside the workspace.
    #[serde(default)]
    pub tabs: Vec<Tab>,
}

/// A single tab inside a layout.
#[derive(Debug, Deserialize, Serialize)]
pub struct Tab {
    /// Label shown on the tab bar.
    pub title: String,

    /// Panes inside this tab.
    #[serde(default)]
    pub panes: Vec<Pane>,
}

/// A pane inside a tab.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pane {
    /// Display label for the pane.
    pub title: Option<String>,

    /// Shell command to execute (mutually exclusive with `agent`).
    pub command: Option<String>,

    /// Agent kind to start (e.g. "claude", "codex").
    pub agent: Option<String>,

    /// Stable agent name passed to `herdr agent start <NAME>`.
    pub agent_name: Option<String>,

    /// Extra arguments forwarded to the agent executable.
    pub agent_args: Option<Vec<String>>,

    /// Prompt text submitted once the agent is ready.
    pub prompt: Option<String>,

    /// Split direction relative to the previous pane: "vertical" or "horizontal".
    pub split: Option<String>,

    /// Pane size along the split axis, e.g. "40%" or "0.4".
    pub size: Option<String>,

    /// Whether the pane stays at the prompt after the command exits.
    pub persist: Option<bool>,
}

/// Maps a workspace (by path or repo name) to a layout.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceMatch {
    /// Prefix-match against the workspace working directory.
    pub path: Option<String>,

    /// Match against the git repository root name.
    pub repo: Option<String>,

    /// Layout id to apply when this workspace matches.
    pub default_layout: Option<String>,
}

impl Pane {
    /// Create an empty pane (just a shell prompt).
    pub fn empty() -> Self {
        Pane {
            title: Some("terminal".to_string()),
            command: None,
            agent: None,
            agent_name: None,
            agent_args: None,
            prompt: None,
            split: None,
            size: None,
            persist: None,
        }
    }
}

impl Config {
    /// Load and parse the config from a YAML file.
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
        serde_yaml::from_str(&content)
            .map_err(|e| format!("failed to parse {}: {}", path.display(), e))
    }

    /// Find a layout by id.
    pub fn find_layout(&self, id: &str) -> Option<&Layout> {
        self.layouts.iter().find(|l| l.id == id)
    }

    /// Resolve which layout to apply for a given workspace directory.
    ///
    /// Checks `workspaces` entries first (path prefix match), then falls back
    /// to `globalLayout`.
    pub fn resolve_layout(&self, workspace_cwd: &str) -> Option<&Layout> {
        // Expand ~ in workspace_cwd for comparison.
        let cwd = workspace_cwd.to_string();

        // Try workspace-specific matches first.
        for ws in &self.workspaces {
            if let Some(ref ws_path) = ws.path {
                let expanded = expand_tilde(ws_path);
                if cwd.starts_with(&expanded) {
                    if let Some(ref layout_id) = ws.default_layout {
                        if let Some(layout) = self.find_layout(layout_id) {
                            return Some(layout);
                        }
                    }
                }
            }
            // TODO: repo matching via git root detection.
        }

        // Fall back to globalLayout.
        if let Some(ref global_id) = self.global_layout {
            return self.find_layout(global_id);
        }

        None
    }

    /// Validate the config and return a list of warnings.
    pub fn validate(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.layouts.is_empty() {
            warnings.push("no layouts defined".to_string());
        }

        // Check globalLayout references a real layout.
        if let Some(ref id) = self.global_layout {
            if self.find_layout(id).is_none() {
                warnings.push(format!(
                    "globalLayout \"{}\" does not match any layout id",
                    id
                ));
            }
        }

        // Check workspace defaultLayout references exist.
        for ws in &self.workspaces {
            if let Some(ref id) = ws.default_layout {
                if self.find_layout(id).is_none() {
                    let target = ws
                        .path
                        .as_deref()
                        .or(ws.repo.as_deref())
                        .unwrap_or("(unnamed)");
                    warnings.push(format!(
                        "workspace \"{}\" references layout \"{}\" which does not exist",
                        target, id
                    ));
                }
            }
        }

        // Check each layout has at least one tab.
        for layout in &self.layouts {
            if layout.tabs.is_empty() {
                warnings.push(format!("layout \"{}\" has no tabs", layout.id));
            }
            for tab in &layout.tabs {
                if tab.panes.is_empty() {
                    warnings.push(format!(
                        "layout \"{}\", tab \"{}\" has no panes",
                        layout.id, tab.title
                    ));
                }
            }
        }

        warnings
    }
}

/// Expand a leading `~` to the user's home directory.
fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('~') {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{}{}", home, rest);
        }
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return format!("{}{}", profile, rest);
        }
    }
    path.to_string()
}
