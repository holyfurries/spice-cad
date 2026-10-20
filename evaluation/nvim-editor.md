# Neovim preferences in the native editor

The reference configuration is `~/.config/nvim/lua/config/settings.lua`,
`config/remap.lua` and the Telescope, Oil, LSP and Trouble plugin configurations.
The app implements the following directly in Rust. It neither runs Neovim nor
executes or watches its configuration files.

## Control-key routing

Vim control keys take priority while the source editor has focus. Ctrl+A/X
change integers in Normal and Visual modes, including counts, signed decimals,
binary/hexadecimal prefixes, and padding. Visual changes use one undo step.
Ctrl+E/Y scroll lines, Ctrl+F/B move pages, and Ctrl+R redoes only in Normal mode.
Insert Ctrl+H/W/U delete text; Ctrl+J/M add a newline; Ctrl+N/P use LSP completion.
Ctrl+C and Ctrl+[ leave Insert or Visual mode. Ctrl+P opens Quick Open only in
Normal mode, preserving the imported mapping.

The window integration emits Cut/Copy events for Ctrl+X/C instead of key events;
those paths also honor Vim mode. The raw-input hook tracks modifier changes in
event order, including a Ctrl press and release within one frame; it leaves
other fields and Standard mode untouched. Unsupported control commands report their
status instead of invoking desktop select-all, cut, or undo. Insert Ctrl+A
replay, block selection, sequential `g` number changes, and jump lists remain
unsupported. System clipboard paste is Ctrl+Shift+V.

File Open/New use Ctrl+Shift+O/N so they cannot steal Vim jump or completion
keys. Find/Replace use Ctrl+Shift+F/H; Standard editor mode also accepts Ctrl+F/H.
Menu actions and the command palette remain available.

Reference behavior follows Vim's [number commands](https://vimhelp.org/change.txt.html#CTRL-A),
[Insert controls](https://vimhelp.org/insert.txt.html#ins-special-keys), and
[scrolling controls](https://vimhelp.org/scroll.txt.html). Headless Neovim probes
confirmed ordinary signed, padded, hexadecimal, binary, fractional-component,
and Visual-selection examples. Regression tests cover native clipboard events,
mode isolation, preserved Standard-mode shortcuts, scrolling, and grouped undo.

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

The default source is the four-part spice rack, copied once to the user data
directory. Examples can be reopened from the top menu without losing local edits.
Display settings live in View → Appearance and the viewport's Display menu;
Properties is optional and starts open beside the 80-column editor.

The control-key revision passes 166 workspace tests, all-target/all-feature
Clippy, formatting, and the native development build. Native checks exercise
Ctrl+A/X, Ctrl+H, `u`/Ctrl+R, Ctrl+Y/Z isolation, page scrolling, Shift+Open/Replace,
and Insert-mode LSP completion. The private test source returns byte-for-byte to
its initial contents. Logs and captures are in `output/native/vim-controls/`.
