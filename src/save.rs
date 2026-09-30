//! Save the current workspace layout as a reusable YAML template.
//!
//! Queries herdr for the workspace's tabs, panes, splits, and running processes,
//! then reconstructs a [`Layout`](crate::config::Layout) and appends it to the
//! plugin config file.

use crate::config::{Layout, Pane, Tab};
use crate::herdr;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

/// Shell process names that indicate an empty/idle pane.
const SHELL_PROCESSES: &[&str] = &[
    "pwsh.exe",
    "pwsh",
    "powershell.exe",
    "powershell",
    "bash",
    "sh",
    "zsh",
    "fish",
    "nu",
    "cmd.exe",
    "cmd",
    "login",
];

/// Known agent binary names mapped to their herdr agent kind.
const AGENT_MAP: &[(&str, &str)] = &[
    ("claude.exe", "claude"),
    ("claude", "claude"),
    ("codex", "codex"),
    ("codex.exe", "codex"),
    ("gemini", "gemini"),
    ("cursor", "cursor"),
    ("devin", "devin"),
    ("agy", "agy"),
    ("cline", "cline"),
    ("opencode", "opencode"),
    ("copilot", "copilot"),
    ("amp", "amp"),
    ("kiro", "kiro"),
    ("grok", "grok"),
];

/// Save the current workspace as a layout template.
pub fn save_workspace(workspace_id: &str, layout_name: &str) -> Result<(), String> {
    eprintln!(
        "[auto-layout] saving workspace {} as layout \"{}\"...",
        workspace_id, layout_name
    );

    // 1. Get tabs
    let tabs_json = herdr::tab_list(workspace_id)?;
    let tabs_arr = tabs_json
        .pointer("/result/tabs")
        .and_then(|v| v.as_array())
        .ok_or("failed to list tabs")?
        .clone();

    // 2. Get all panes, grouped by tab_id
    let panes_json = herdr::pane_list(workspace_id)?;
    let panes_arr = panes_json
        .pointer("/result/panes")
        .and_then(|v| v.as_array())
        .ok_or("failed to list panes")?
        .clone();

    let mut panes_by_tab: HashMap<String, Vec<Value>> = HashMap::new();
    for pane in &panes_arr {
        let tab_id = pane
            .pointer("/tab_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        panes_by_tab.entry(tab_id).or_default().push(pane.clone());
    }

    // 3. Build layout
    let mut layout_tabs: Vec<Tab> = Vec::new();

    for tab in &tabs_arr {
        let tab_id = tab.pointer("/tab_id").and_then(|v| v.as_str()).unwrap_or("");
        let tab_label = tab
            .pointer("/label")
            .and_then(|v| v.as_str())
            .unwrap_or("unnamed")
            .to_string();

        let tab_panes = panes_by_tab.get(tab_id).cloned().unwrap_or_default();
        if tab_panes.is_empty() {
            layout_tabs.push(Tab {
                title: tab_label,
                panes: vec![Pane::empty()],
            });
            continue;
        }

        // Get layout info (splits) for this tab
        let first_pane_id = tab_panes[0]
            .pointer("/pane_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let layout_info = herdr::pane_layout(first_pane_id).ok();
        let splits = layout_info
            .as_ref()
            .and_then(|l| l.pointer("/result/layout/splits"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Get ordered pane_ids from layout (preserves visual order)
        let ordered_pane_ids: Vec<String> = layout_info
            .as_ref()
            .and_then(|l| l.pointer("/result/layout/panes"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| p.pointer("/pane_id").and_then(|v| v.as_str()))
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_else(|| {
                tab_panes
                    .iter()
                    .filter_map(|p| p.pointer("/pane_id").and_then(|v| v.as_str()))
                    .map(|s| s.to_string())
                    .collect()
            });

        // Build pane index for quick lookup
        let pane_index: HashMap<String, &Value> = tab_panes
            .iter()
            .filter_map(|p| {
                p.pointer("/pane_id")
                    .and_then(|v| v.as_str())
                    .map(|id| (id.to_string(), p))
            })
            .collect();

        let mut built_panes: Vec<Pane> = Vec::new();

        for (pane_idx, pane_id) in ordered_pane_ids.iter().enumerate() {
            let pane_data = pane_index.get(pane_id.as_str());

            // Detect agent from pane list data
            let agent_kind = pane_data
                .and_then(|p| p.pointer("/agent"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            // Get process info to detect command or agent args
            let proc_info = herdr::pane_process_info(pane_id).ok();
            let fg_process = proc_info
                .as_ref()
                .and_then(|p| p.pointer("/result/process_info/foreground_processes/0"));

            let process_name = fg_process
                .and_then(|p| p.pointer("/name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let process_argv: Vec<String> = fg_process
                .and_then(|p| p.pointer("/argv"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect()
                })
                .unwrap_or_default();

            // Determine split direction and ratio for non-first panes
            let (split, size) = if pane_idx > 0 && pane_idx - 1 < splits.len() {
                let split_info = &splits[pane_idx - 1];
                let dir = split_info
                    .pointer("/direction")
                    .and_then(|v| v.as_str())
                    .unwrap_or("right");
                let ratio = split_info
                    .pointer("/ratio")
                    .and_then(|v| v.as_f64());

                let direction = match dir {
                    "down" => "horizontal",
                    _ => "vertical",
                };

                let size_str = ratio.map(|r| {
                    // The second pane's share is (1 - ratio), expressed as percentage
                    let pct = ((1.0 - r) * 100.0).round() as u32;
                    format!("{}%", pct)
                });

                (Some(direction.to_string()), size_str)
            } else {
                (None, None)
            };

            // Build the pane config
            let pane = if let Some(ref kind) = agent_kind {
                // Agent pane — extract args from argv (skip binary path)
                let agent_args: Vec<String> = if process_argv.len() > 1 {
                    process_argv[1..].to_vec()
                } else {
                    Vec::new()
                };

                Pane {
                    title: Some(kind.clone()),
                    command: None,
                    agent: Some(kind.clone()),
                    agent_name: None,
                    agent_args: if agent_args.is_empty() {
                        None
                    } else {
                        Some(agent_args)
                    },
                    prompt: None,
                    split,
                    size,
                    persist: None,
                }
            } else if !process_name.is_empty() && !is_shell_process(process_name) {
                // Command pane
                let cmd = detect_command(process_name, &process_argv);
                Pane {
                    title: Some(cmd.clone()),
                    command: Some(cmd),
                    agent: None,
                    agent_name: None,
                    agent_args: None,
                    prompt: None,
                    split,
                    size,
                    persist: None,
                }
            } else {
                // Empty shell pane
                Pane {
                    title: Some("terminal".to_string()),
                    command: None,
                    agent: None,
                    agent_name: None,
                    agent_args: None,
                    prompt: None,
                    split,
                    size,
                    persist: None,
                }
            };

            built_panes.push(pane);
        }

        layout_tabs.push(Tab {
            title: tab_label,
            panes: built_panes,
        });
    }

    let layout = Layout {
        id: layout_name.to_string(),
        tabs: layout_tabs,
    };

    // 4. Serialize and append to config
    let yaml = serialize_layout(&layout)?;
    eprintln!("[auto-layout] generated layout:\n{}", yaml);

    append_layout_to_config(&layout)?;
    eprintln!(
        "[auto-layout] layout \"{}\" saved to config.yaml ✓",
        layout_name
    );

    Ok(())
}

/// Check if a process name is a shell (idle pane).
fn is_shell_process(name: &str) -> bool {
    let lower = name.to_lowercase();
    SHELL_PROCESSES.iter().any(|s| lower == *s)
}

/// Detect the command name from process info.
/// Uses the short name, stripping .exe suffix and path prefixes.
fn detect_command(process_name: &str, argv: &[String]) -> String {
    // Check if this is a known agent (shouldn't reach here, but just in case)
    let short = process_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(process_name);

    for &(bin, _kind) in AGENT_MAP {
        if short.eq_ignore_ascii_case(bin) {
            // Return the full command with args
            if argv.len() > 1 {
                return argv[1..].join(" ");
            }
            return short.to_string();
        }
    }

    // Return just the short process name (e.g. "nvim", "lazygit")
    let clean = short.strip_suffix(".exe").unwrap_or(short);
    clean.to_string()
}

/// Serialize a Layout to YAML string.
fn serialize_layout(layout: &Layout) -> Result<String, String> {
    let mut yaml = format!("  - id: {}\n    tabs:\n", layout.id);

    for tab in &layout.tabs {
        yaml.push_str(&format!("      - title: {}\n        panes:\n", tab.title));

        for (i, pane) in tab.panes.iter().enumerate() {
            yaml.push_str("          - ");

            let mut fields: Vec<String> = Vec::new();

            if let Some(ref title) = pane.title {
                fields.push(format!("title: {}", title));
            }

            if let Some(ref agent) = pane.agent {
                fields.push(format!("agent: {}", agent));
                if let Some(ref args) = pane.agent_args {
                    if !args.is_empty() {
                        fields.push("agentArgs:".to_string());
                    }
                }
            } else if let Some(ref cmd) = pane.command {
                fields.push(format!("command: {}", cmd));
            }

            if i > 0 {
                if let Some(ref split) = pane.split {
                    fields.push(format!("split: {}", split));
                }
            }

            if let Some(ref size) = pane.size {
                fields.push(format!("size: \"{}\"", size));
            }

            // Write first field on the same line as `-`
            if let Some(first) = fields.first() {
                yaml.push_str(first);
                yaml.push('\n');
            }

            // Write remaining fields indented
            for field in fields.iter().skip(1) {
                yaml.push_str(&format!("            {}\n", field));
            }

            // Write agentArgs list items
            if let Some(ref agent_args) = pane.agent_args {
                if pane.agent.is_some() && !agent_args.is_empty() {
                    for arg in agent_args {
                        yaml.push_str(&format!("              - \"{}\"\n", arg));
                    }
                }
            }
        }
    }

    Ok(yaml)
}

/// Append a layout to the existing config.yaml file.
fn append_layout_to_config(layout: &Layout) -> Result<(), String> {
    let config_dir = std::env::var("HERDR_PLUGIN_CONFIG_DIR")
        .map_err(|_| "HERDR_PLUGIN_CONFIG_DIR not set".to_string())?;

    let config_path = PathBuf::from(&config_dir).join("config.yaml");

    // Read existing content
    let existing = if config_path.exists() {
        std::fs::read_to_string(&config_path)
            .map_err(|e| format!("failed to read config: {}", e))?
    } else {
        "globalLayout: default\n\nlayouts:\n".to_string()
    };

    // Check if a layout with this id already exists
    if existing.contains(&format!("id: {}", layout.id)) {
        return Err(format!(
            "layout \"{}\" already exists in config.yaml — choose a different name",
            layout.id
        ));
    }

    // Append the new layout under the layouts key
    let yaml = serialize_layout(layout)?;

    let new_content = if existing.contains("layouts:") {
        // Append after existing layouts
        format!("{}\n{}", existing.trim_end(), yaml)
    } else {
        // Add layouts section
        format!("{}\nlayouts:\n{}", existing.trim_end(), yaml)
    };

    std::fs::write(&config_path, new_content)
        .map_err(|e| format!("failed to write config: {}", e))?;

    Ok(())
}
