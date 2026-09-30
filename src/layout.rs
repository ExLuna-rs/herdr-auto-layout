//! Layout application logic.
//!
//! Takes a resolved [`Layout`] and materialises it inside a herdr workspace by
//! creating tabs, splitting panes, starting agents, and running commands.

use crate::config::{Layout, Pane};
use crate::herdr;

/// Apply a layout to an existing workspace.
///
/// The workspace is assumed to be *fresh* — exactly one tab with one empty
/// pane.  The first tab in the layout reuses that existing tab/pane; every
/// subsequent tab is created via `herdr tab create`.
pub fn apply(
    workspace_id: &str,
    first_tab_id: &str,
    first_pane_id: &str,
    layout: &Layout,
) -> Result<(), String> {
    eprintln!(
        "[auto-layout] applying layout \"{}\" ({} tab(s)) to workspace {}",
        layout.id,
        layout.tabs.len(),
        workspace_id,
    );

    for (tab_idx, tab) in layout.tabs.iter().enumerate() {
        let root_pane_id: String;

        if tab_idx == 0 {
            // Reuse the workspace's existing first tab and pane.
            root_pane_id = first_pane_id.to_string();

            // Rename the existing tab to match the layout.
            if let Err(e) = herdr::tab_rename(first_tab_id, &tab.title) {
                eprintln!("[auto-layout] warning: failed to rename first tab: {}", e);
            }
        } else {
            // Create a new tab.
            let result = herdr::tab_create(workspace_id, &tab.title)
                .map_err(|e| format!("failed to create tab \"{}\": {}", tab.title, e))?;
            root_pane_id = result.pane_id;
        }

        // Build panes inside this tab.
        build_panes(&root_pane_id, &tab.panes)?;
    }

    Ok(())
}

/// Create and configure panes inside a tab.
///
/// The first pane reuses `root_pane_id` (already exists).  Subsequent panes
/// are created by splitting from the previous pane.
fn build_panes(root_pane_id: &str, panes: &[Pane]) -> Result<(), String> {
    let mut current_pane_id = root_pane_id.to_string();

    for (pane_idx, pane) in panes.iter().enumerate() {
        if pane_idx == 0 {
            // First pane already exists — just configure it.
            configure_pane(&current_pane_id, pane)?;
        } else {
            // Split from the previous pane.
            let direction = resolve_direction(pane.split.as_deref());
            let ratio = resolve_ratio(pane.size.as_deref());

            let result = herdr::pane_split(&current_pane_id, direction, ratio).map_err(|e| {
                format!(
                    "failed to split pane for \"{}\": {}",
                    pane.title.as_deref().unwrap_or("unnamed"),
                    e
                )
            })?;

            current_pane_id = result.pane_id.clone();
            configure_pane(&result.pane_id, pane)?;
        }
    }

    Ok(())
}

/// Configure a single pane: rename, start agent or run command.
fn configure_pane(pane_id: &str, pane: &Pane) -> Result<(), String> {
    // Rename the pane if a title is set.
    if let Some(ref title) = pane.title {
        let _ = herdr::pane_rename(pane_id, title);
    }

    if let Some(ref agent_kind) = pane.agent {
        // Start an agent.
        let agent_name = pane
            .agent_name
            .as_deref()
            .or(pane.title.as_deref())
            .unwrap_or(agent_kind);

        let empty_args = Vec::new();
        let args = pane.agent_args.as_ref().unwrap_or(&empty_args);

        if let Err(e) = herdr::agent_start(agent_name, agent_kind, pane_id, args) {
            eprintln!(
                "[auto-layout] warning: failed to start agent \"{}\" ({}): {}",
                agent_name, agent_kind, e
            );
        }

        // Optionally submit a prompt after agent start.
        if let Some(ref prompt_text) = pane.prompt {
            if let Err(e) = herdr::agent_prompt(agent_name, prompt_text) {
                eprintln!(
                    "[auto-layout] warning: failed to send prompt to \"{}\": {}",
                    agent_name, e
                );
            }
        }
    } else if let Some(ref cmd) = pane.command {
        // Run a shell command.
        if !cmd.is_empty() {
            if let Err(e) = herdr::pane_send_keys(pane_id, cmd) {
                eprintln!(
                    "[auto-layout] warning: failed to send command to pane: {}",
                    e
                );
            }
        }
    }

    Ok(())
}

/// Map the config split direction to herdr CLI direction.
///
/// - `"vertical"` → `"right"` (side-by-side)
/// - `"horizontal"` → `"down"` (stacked)
/// - Default → `"right"`
fn resolve_direction(split: Option<&str>) -> &'static str {
    match split {
        Some("horizontal") | Some("down") => "down",
        _ => "right", // "vertical", "right", or unset
    }
}

/// Parse a size string like `"40%"` or `"0.4"` into an `f64` ratio.
fn resolve_ratio(size: Option<&str>) -> Option<f64> {
    let s = size?;

    if let Some(pct) = s.strip_suffix('%') {
        if let Ok(n) = pct.trim().parse::<f64>() {
            return Some(n / 100.0);
        }
    }

    if let Ok(f) = s.parse::<f64>() {
        if (0.0..=1.0).contains(&f) {
            return Some(f);
        }
    }

    None
}
