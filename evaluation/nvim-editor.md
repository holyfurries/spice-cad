# Neovim preferences in the native editor

The reference configuration is `~/.config/nvim/lua/config/settings.lua`,
`config/remap.lua` and the Telescope, Oil, LSP and Trouble plugin configurations.
The app implements the following directly in Rust. It neither runs Neovim nor
executes or watches its configuration files.

| Preference | Native behavior |
| --- | --- |
| `colorcolumn=80`, `wrap=false` | An 80-column guide and an initial pane width calculated from the monospace font and gutter. Long lines scroll horizontally; resizing remains free. |
| `number`, `relativenumber` | Current absolute line number with relative numbers around it. |
| Four-space tabs and `smartindent` | Tab advances to a four-space boundary. Enter preserves indentation and adds one level after common Luau block openings; indentation work is bounded. |
| `scrolloff=8` | Eight-line cursor margin, reduced in short viewports. |
| Ctrl+D / Ctrl+U with `zz` | Half-page movement with centered cursor reveal in Normal mode. |
| Insert-mode Ctrl+C | Escape to Normal mode, including the desktop clipboard-copy event path. |
| Space P F, Ctrl+P | Quick Open: bounded recursive fuzzy search of model files, recent files first. |
| Oil `-` | Open the native file browser at the current model's directory. |
| Space V D / Space X X | Toggle the diagnostics panel. |
| Ctrl+K / Ctrl+J | Next / previous located diagnostic, wrapping at the ends. Ctrl+Shift+J toggles the panel globally. |
| Space Space | Adapted to Run the current CAD source. |

Existing Tree-sitter highlighting, LSP completion, `gd`, `K`, modal edits and rope
undo remain integrated with the same buffer. The app retains the CAD charcoal
palette rather than importing the complete Hypersubatomic theme.

The plugin runtime, tmux sessionizer, OSC 52 terminal clipboard, shell execution,
Git integration, persistent undo files, Harpoon, Telescope search, snippets, LSP
rename/references/code actions, and the remaining custom editing operators are
not imported. Desktop clipboard operations use the native window integration.
Formatting on save is not added: the supplied formatter map has no Luau formatter.
Neovim's `updatetime=50` is not used as a polling interval: the native UI remains
event-driven, with the existing debounced language-server worker.

The default source is the assembled rack blank, copied once to the user data
directory. Examples can be reopened from the top menu without losing local edits.
Display settings live in the top Display menu; Properties is optional and starts
collapsed to leave room beside the 80-column editor.

Validation passes 44 workspace tests, default and all-feature Clippy, formatting
and the release build. Native checks cover both examples, the Display menu,
leader file/diagnostic actions, smart indentation, Ctrl+C, half-page movement,
next/previous diagnostics and the global panel shortcut. Results are recorded in
`output/native/nvim-qa-results.log` and `output/native/nvim-keys-qa-results.log`.
The rack passes native solid/mesh validation with volume 972,678 mm³.
