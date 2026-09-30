//! Wrapper around the herdr CLI.
//!
//! Every function shells out to `herdr` (resolved via `HERDR_BIN_PATH`) and
//! parses the JSON response.

use serde_json::Value;
use std::process::Command;

/// Resolve the herdr binary path from the environment.
fn herdr_bin() -> String {
    std::env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".to_string())
}

/// Run a herdr command and return the parsed JSON output.
fn run(args: &[&str]) -> Result<Value, String> {
    let bin = herdr_bin();
    let output = Command::new(&bin)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run `{} {}`: {}", bin, args.join(" "), e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`{} {}` failed (exit {}): {}",
            bin,
            args.join(" "),
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .map_err(|e| format!("failed to parse herdr output: {} — raw: {}", e, stdout.trim()))
}

/// Run a herdr command, ignoring the output.  Returns Ok on exit 0.
fn run_quiet(args: &[&str]) -> Result<(), String> {
    let bin = herdr_bin();
    let output = Command::new(&bin)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run `{} {}`: {}", bin, args.join(" "), e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`{} {}` failed (exit {}): {}",
            bin,
            args.join(" "),
            output.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tab operations
// ---------------------------------------------------------------------------

/// Result of creating a tab.
pub struct TabCreateResult {
    pub pane_id: String,
}

/// Create a new tab in the given workspace.
pub fn tab_create(workspace_id: &str, label: &str) -> Result<TabCreateResult, String> {
    let json = run(&[
        "tab",
        "create",
        "--workspace",
        workspace_id,
        "--label",
        label,
        "--no-focus",
    ])?;

    let pane_id = json
        .pointer("/result/root_pane/pane_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "tab create: missing result.root_pane.pane_id in response".to_string())?
        .to_string();

    Ok(TabCreateResult { pane_id })
}

/// Rename an existing tab.
pub fn tab_rename(tab_id: &str, label: &str) -> Result<(), String> {
    run_quiet(&["tab", "rename", tab_id, "--label", label])
}

// ---------------------------------------------------------------------------
// Pane operations
// ---------------------------------------------------------------------------

/// Result of splitting a pane.
pub struct PaneSplitResult {
    pub pane_id: String,
}

/// Split an existing pane in the given direction.
///
/// `direction` must be `"right"` or `"down"`.
/// `ratio` is a float between 0 and 1 representing the new pane's share.
pub fn pane_split(
    pane_id: &str,
    direction: &str,
    ratio: Option<f64>,
) -> Result<PaneSplitResult, String> {
    let mut args = vec!["pane", "split", pane_id, "--direction", direction, "--no-focus"];

    let ratio_str;
    if let Some(r) = ratio {
        ratio_str = format!("{:.2}", r);
        args.push("--ratio");
        args.push(&ratio_str);
    }

    let json = run(&args)?;

    let new_pane_id = json
        .pointer("/result/pane/pane_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "pane split: missing result.pane.pane_id in response".to_string())?
        .to_string();

    Ok(PaneSplitResult {
        pane_id: new_pane_id,
    })
}

/// Run a command in a pane (sends text + Enter).
pub fn pane_run_command(pane_id: &str, command: &str) -> Result<(), String> {
    run_quiet(&["pane", "run", pane_id, command])
}

/// Rename a pane.
pub fn pane_rename(pane_id: &str, label: &str) -> Result<(), String> {
    run_quiet(&["pane", "rename", pane_id, "--label", label])
}

// ---------------------------------------------------------------------------
// Agent operations
// ---------------------------------------------------------------------------

/// Start an agent in the given pane.
///
/// `name` is the stable alias, `kind` is the agent type (e.g. "claude"),
/// `extra_args` are forwarded after `--`.
pub fn agent_start(
    name: &str,
    kind: &str,
    pane_id: &str,
    extra_args: &[String],
) -> Result<(), String> {
    let mut args: Vec<&str> = vec!["agent", "start", name, "--kind", kind, "--pane", pane_id];

    if !extra_args.is_empty() {
        args.push("--");
        for a in extra_args {
            args.push(a.as_str());
        }
    }

    run_quiet(&args)
}

/// Submit a prompt to a running agent.
pub fn agent_prompt(name: &str, text: &str) -> Result<(), String> {
    run_quiet(&["agent", "prompt", name, text])
}

/// Get the first pane_id of a given tab by listing panes.
pub fn get_first_pane_of_tab(tab_id: &str) -> Result<String, String> {
    let json = run(&["pane", "list", "--tab", tab_id])?;

    json.pointer("/result/panes/0/pane_id")
        .or_else(|| json.pointer("/result/0/pane_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("no panes found for tab {}", tab_id))
}
