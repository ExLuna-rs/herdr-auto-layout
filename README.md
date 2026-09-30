# herdr-auto-layout

Declarative layouts applied to **every new workspace** — cross-platform herdr plugin.

Unlike workspace-manager (which only targets git worktrees), this plugin applies
layouts automatically whenever a new workspace is created, whether from a
worktree, a manual `prefix+shift+n`, or the CLI.

## Features

- **Auto-apply on `workspace.created`** — every new workspace gets a layout
- **Manual apply** — trigger via keybinding or `herdr plugin action invoke`
- **Cross-platform** — works on Linux, macOS, and Windows
- **Declarative YAML config** — define tabs, panes, agents, and commands
- **Workspace matching** — different layouts for different project directories
- **Agent support** — auto-start Claude, Codex, and other agents with custom args

## Installation

### From source (requires Rust)

```bash
git clone https://github.com/ExLuna-rs/herdr-auto-layout.git
herdr plugin link ./herdr-auto-layout
```

The plugin builds itself on first use via `cargo build --release`.

### From GitHub

```bash
herdr plugin install ExLuna-rs/herdr-auto-layout
```

## Configuration

Create `config.yaml` in the plugin config directory:

```bash
# Find the config directory
herdr plugin config-dir auto-layout

# Create the config
mkdir -p "$(herdr plugin config-dir auto-layout)"
$EDITOR "$(herdr plugin config-dir auto-layout)/config.yaml"
```

### Example config

```yaml
globalLayout: default

layouts:
  - id: default
    tabs:
      - title: dev
        panes:
          - title: agent
            agent: claude
            agentName: main
            agentArgs:
              - "--model"
              - "opus5.5"
            size: "60%"
          - title: editor
            command: nvim
            split: vertical

      - title: shell
        panes:
          - title: terminal

      - title: git
        panes:
          - title: lazygit
            command: lazygit
            size: "65%"
          - title: dashboard
            command: gh dash
            split: horizontal

  - id: minimal
    tabs:
      - title: shell
        panes:
          - title: terminal

workspaces:
  - path: ~/projects/my-web-app
    defaultLayout: default
  - path: ~/scratch
    defaultLayout: minimal
```

### Config reference

#### Top level

| Field          | Type     | Description                                         |
| -------------- | -------- | --------------------------------------------------- |
| `globalLayout` | string   | Fallback layout id when no workspace match is found |
| `layouts`      | Layout[] | Available layout definitions                        |
| `workspaces`   | Match[]  | Optional workspace-to-layout mappings               |

#### Layout

| Field  | Type  | Description                           |
| ------ | ----- | ------------------------------------- |
| `id`   | string | Unique identifier                    |
| `tabs` | Tab[] | Tabs to create inside the workspace  |

#### Tab

| Field   | Type   | Description          |
| ------- | ------ | -------------------- |
| `title` | string | Tab bar label        |
| `panes` | Pane[] | Panes inside the tab |

#### Pane

| Field       | Type     | Description                                             |
| ----------- | -------- | ------------------------------------------------------- |
| `title`     | string?  | Pane label                                              |
| `command`   | string?  | Shell command (mutually exclusive with `agent`)          |
| `agent`     | string?  | Agent kind: claude, codex, gemini, etc.                 |
| `agentName` | string?  | Stable agent alias                                      |
| `agentArgs` | string[] | Extra args forwarded to the agent                       |
| `prompt`    | string?  | Text submitted once the agent is ready                  |
| `split`     | string?  | `"vertical"` (right) or `"horizontal"` (down)           |
| `size`      | string?  | Pane share: `"40%"`, `"0.4"`, etc.                      |
| `persist`   | bool?    | Keep prompt after command exits (reserved, not yet used) |

#### Workspace match

| Field           | Type    | Description                                |
| --------------- | ------- | ------------------------------------------ |
| `path`          | string? | Prefix-match on workspace working directory |
| `repo`          | string? | Match on git repo root name (planned)       |
| `defaultLayout` | string? | Layout id to apply                          |

## Keybindings

Add to your herdr `config.toml`:

```toml
[[keys.command]]
key = "prefix+shift+l"
type = "plugin_action"
command = "auto-layout.apply"
description = "apply layout"

[[keys.command]]
key = "prefix+shift+v"
type = "plugin_action"
command = "auto-layout.validate"
description = "validate layout config"
```

## How it works

1. You create a new workspace (keybinding, CLI, or TUI)
2. Herdr fires `workspace.created`
3. The plugin reads your `config.yaml`
4. It matches the workspace's cwd against `workspaces[]` entries
5. Falls back to `globalLayout` if no match
6. Creates tabs, splits panes, starts agents, and runs commands

The first tab/pane of the layout reuses the workspace's existing empty tab —
no duplicates.

## Requirements

- herdr ≥ 0.8.0
- Rust toolchain (for building from source)

## Roadmap

### 🖥️ Adaptive display scaling
Automatically adjust font size and pane proportions based on screen size. On smaller screens (14" laptops), herdr can feel cramped — the plugin will detect terminal dimensions and apply optimized ratios/font settings per layout.

### 💾 Save current workspace as template
Capture a manually configured workspace (tabs, panes, splits, commands) and save it as a reusable layout template. Then apply it automatically to new workspaces.

```bash
# Planned usage:
herdr plugin action invoke save --plugin auto-layout
# → Saves current workspace layout as a new template in config.yaml
```

## License

MIT
