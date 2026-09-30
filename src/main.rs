//! herdr-auto-layout — declarative layouts for every new workspace.
//!
//! A cross-platform herdr plugin that listens to `workspace.created` events and
//! automatically applies a layout (tabs, panes, agents, commands) declared in a
//! YAML config file.
//!
//! # Subcommands
//!
//! - `startup`  — runs once when the herdr server starts (no-op, reserved).
//! - `on-event` — triggered by `workspace.created`; applies the matching layout.
//! - `apply`    — manually apply a layout to the focused workspace.
//! - `validate` — parse and validate the config, printing a summary.

mod config;
mod herdr;
mod layout;
mod save;

use config::Config;
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let subcommand = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    let result = match subcommand {
        "startup" => cmd_startup(),
        "on-event" => cmd_on_event(),
        "apply" => cmd_apply(),
        "save" => cmd_save(),
        "validate" => cmd_validate(),
        _ => {
            eprintln!("usage: herdr-auto-layout <startup|on-event|apply|save|validate>");
            std::process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("[auto-layout] error: {}", e);
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

/// Startup hook — reserved for future state cleanup.
fn cmd_startup() -> Result<(), String> {
    eprintln!("[auto-layout] startup: ready");
    Ok(())
}

/// Event handler for `workspace.created`.
///
/// Reads `HERDR_PLUGIN_EVENT_JSON` to discover the new workspace, resolves the
/// matching layout from the config, and applies it.
fn cmd_on_event() -> Result<(), String> {
    let event_json = std::env::var("HERDR_PLUGIN_EVENT_JSON")
        .map_err(|_| "HERDR_PLUGIN_EVENT_JSON not set".to_string())?;

    let event: serde_json::Value = serde_json::from_str(&event_json)
        .map_err(|e| format!("failed to parse event JSON: {}", e))?;

    // The event JSON structure is: { "event": "workspace_created", "data": { "workspace": { ... } } }
    let workspace_id = event
        .pointer("/data/workspace/workspace_id")
        .or_else(|| event.pointer("/workspace/workspace_id"))
        .or_else(|| event.pointer("/workspace_id"))
        .and_then(|v| v.as_str())
        .ok_or("event JSON missing workspace_id")?
        .to_string();

    let workspace_cwd = std::env::var("HERDR_ACTIVE_PANE_CWD")
        .or_else(|_| {
            event
                .pointer("/data/workspace/cwd")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or(std::env::VarError::NotPresent)
        })
        .unwrap_or_default();

    let tab_id = event
        .pointer("/data/workspace/active_tab_id")
        .or_else(|| event.pointer("/tab/tab_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| std::env::var("HERDR_TAB_ID").ok())
        .unwrap_or_default();

    // For a fresh workspace the pane_id isn't in the event, so we use env vars
    // or query herdr for the first pane in the tab.
    let pane_id = std::env::var("HERDR_PANE_ID")
        .or_else(|_| std::env::var("HERDR_ACTIVE_PANE_ID"))
        .or_else(|_| {
            // Derive from tab_id: query herdr for panes in this tab
            herdr::get_first_pane_of_tab(&tab_id)
        })
        .unwrap_or_default();

    if pane_id.is_empty() || tab_id.is_empty() {
        return Err(format!(
            "cannot determine tab_id ({}) or pane_id ({}) — skipping layout",
            tab_id, pane_id
        ));
    }

    let config = load_config()?;
    let layout = config
        .resolve_layout(&workspace_cwd)
        .ok_or("no matching layout found for this workspace")?;

    layout::apply(&workspace_id, &tab_id, &pane_id, layout)
}

/// Manual apply: apply a layout to the currently focused workspace.
///
/// Reads workspace context from `HERDR_PLUGIN_CONTEXT_JSON` or individual
/// environment variables.
fn cmd_apply() -> Result<(), String> {
    let workspace_id = get_context_field("workspace_id", "/workspace_id")
        .or_else(|_| std::env::var("HERDR_WORKSPACE_ID"))
        .map_err(|_| "could not determine workspace id from context".to_string())?;

    let workspace_cwd = get_context_field("workspace_cwd", "/workspace_cwd").unwrap_or_default();

    let tab_id = get_context_field("tab_id", "/tab_id")
        .or_else(|_| std::env::var("HERDR_TAB_ID"))
        .map_err(|_| "could not determine tab id from context".to_string())?;

    let pane_id = get_context_field("focused_pane_id", "/focused_pane_id")
        .or_else(|_| std::env::var("HERDR_PANE_ID"))
        .map_err(|_| "could not determine pane id from context".to_string())?;

    let config = load_config()?;
    let layout = config
        .resolve_layout(&workspace_cwd)
        .ok_or("no matching layout found for this workspace")?;

    layout::apply(&workspace_id, &tab_id, &pane_id, layout)
}

/// Validate the config and print a summary.
fn cmd_validate() -> Result<(), String> {
    let config = load_config()?;
    let warnings = config.validate();

    eprintln!("[auto-layout] config validation:");
    eprintln!("  layouts: {}", config.layouts.len());
    for l in &config.layouts {
        let tab_count = l.tabs.len();
        let pane_count: usize = l.tabs.iter().map(|t| t.panes.len()).sum();
        eprintln!(
            "    - \"{}\" ({} tab(s), {} pane(s))",
            l.id, tab_count, pane_count
        );
    }
    eprintln!(
        "  globalLayout: {}",
        config
            .global_layout
            .as_deref()
            .unwrap_or("(not set)")
    );
    eprintln!("  workspace mappings: {}", config.workspaces.len());

    if warnings.is_empty() {
        eprintln!("  status: ok ✓");
    } else {
        eprintln!("  warnings:");
        for w in &warnings {
            eprintln!("    ⚠ {}", w);
        }
    }

    Ok(())
}

/// Save the current workspace layout as a named template.
///
/// Captures tabs, panes, splits, agents, and commands, then appends the
/// resulting layout to config.yaml.
fn cmd_save() -> Result<(), String> {
    let workspace_id = get_context_field("workspace_id", "/workspace_id")
        .or_else(|_| std::env::var("HERDR_WORKSPACE_ID"))
        .map_err(|_| "could not determine workspace id from context".to_string())?;

    // Use a timestamp-based name; users can rename in config.yaml later.
    let name = format!(
        "saved-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    );

    save::save_workspace(&workspace_id, &name)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Load the plugin config from `$HERDR_PLUGIN_CONFIG_DIR/config.yaml`.
fn load_config() -> Result<Config, String> {
    let config_dir = std::env::var("HERDR_PLUGIN_CONFIG_DIR")
        .map_err(|_| "HERDR_PLUGIN_CONFIG_DIR not set".to_string())?;

    let path = PathBuf::from(&config_dir).join("config.yaml");

    if !path.exists() {
        return Err(format!(
            "config file not found at {} — create it to define your layouts",
            path.display()
        ));
    }

    Config::load(&path)
}

/// Extract a field from `HERDR_PLUGIN_CONTEXT_JSON`.
fn get_context_field(field_name: &str, json_pointer: &str) -> Result<String, String> {
    let ctx_json = std::env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .map_err(|_| format!("HERDR_PLUGIN_CONTEXT_JSON not set (looking for {})", field_name))?;

    let ctx: serde_json::Value = serde_json::from_str(&ctx_json)
        .map_err(|e| format!("failed to parse context JSON: {}", e))?;

    ctx.pointer(json_pointer)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("context JSON missing field {}", field_name))
}
