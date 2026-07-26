---
title: Implement `dotcfg-gui` — extensible GTK4/libadwaita dotfile config editor with Niri as v1 driver
---

## Initial User Prompt

Build a GUI editor for the niri Wayland compositor's config.kdl file. Goal: a clean, single-binary GTK4/libadwaita GUI in Rust for editing `~/.config/niri/config.kdl`. The architecture must be designed so future dotfile/config editors (sway, hyprland, waybar, kitty, alacritty, fish, etc.) can be added as additional tool modules without rewriting the shell. Stack: Rust + gtk4-rs + libadwaita. Validation: shell out to `niri msg validate` rather than reimplementing niri's parser. Round-trip safety is mandatory — preserve user comments, whitespace, and ordering on save.

## Description

### Problem statement

Niri, sway, hyprland, and the broader Linux tiling-WM ecosystem store configuration in plain-text files (KDL, INI, TOML, YAML). Users edit these files by hand, with high error rates: missing closing braces, wrong key names, keybindings that conflict, layouts that ship malformed. Tooling is "open in vim + a CLI validator". There is no first-class GUI editor that respects each format's structure (preserves comments, validates via the running compositor, surfaces validation errors with line numbers).

### Primary users

- New Linux/tiling-WM users setting up their first niri install.
- Power users managing multiple dotfiles across multiple machines.
- Users who want validation/hot-reload visibility without leaving a GUI.

### Value proposition

A single Rust binary that gives per-tool, format-aware visual editing with composer-backed validation, comment-preserving round-trip save, hot reload, and per-tool extensibility for any future dotfile format.

### Primary scenarios

1. **First-run, no config** → `Adw.StatusPage` with "Generate Default Config" → click → baseline written → editor opens.
2. **Open existing `~/.config/niri/config.kdl`** → KDL parses, comments preserved; sidebar lists Niri tool; main panel shows per-section tabs.
3. **Edit a bind** → "Record key combo" → modal `Adw.Dialog` captures chord → row value updates → `UndoCommand` pushed → dirty flag set → 250ms debounce → `niri msg validate` → toast/banner updates.
4. **Save** → atomic write (tmp + rename) → niri auto-reloads → toast "Config reloaded".
5. **External edit** (vim modifies file) → `notify::RecommendedWatcher` fires → if clean, auto-reload + toast; if dirty, `Adw.Banner (Reload / Ignore)`.
6. **Quit with dirty state** → `close-request` intercepted → `Adw.AlertDialog (Save / Discard / Cancel)` → only proceed after response.
7. **Corrupt KDL** → parse failure → central view swaps to `GtkSourceView` with offending span highlighted → user fixes → resave → GUI restored.

### In-scope v1

- Niri (config.kdl) editor — full widget suite (10 sections).
- App shell: sidebar + main pane + libadwaita GNOME feel.
- Validation via `niri msg validate` (Flatpak: `flatpak-spawn --host`).
- Hot reload via `notify::RecommendedWatcher`.
- Comment-preserving round-trip KDL.
- Test infra (xvfb + proptest: round-trip + undo commutativity).
- Flatpak manifest + `.desktop` entry.
- Manual pre-release checklist.

### Out-of-scope v1 (YAGNI)

- Save-on-quit snapshots (`~/.config/dotcfg-gui/snapshots/`).
- Multi-tool simultaneous editing (one tool active at a time in v1).
- Dynamic `.so` plugin loading (compile-time via Cargo features is enough).
- Internationalization.
- Differential diff/highlighting during reload conflicts (just Reload/Ignore/AskUser).
- Plugins for sway / hyprland / waybar / kitty / alacritty (today). Architecture supports them; v1 ships Niri only.

### Acceptance criteria

1. App starts via `cargo run`: shows `Adw.ApplicationWindow` with sidebar + main pane.
2. With `~/.config/niri/config.kdl` present: KDL parses; sidebar shows Niri tool; main pane shows section list.
3. With config absent: `Adw.StatusPage` with "Generate Default Config" appears and writes a baseline on click.
4. Editing a bind triggers `niri msg validate` within ≤500ms total (250ms debounce + spawn).
5. Saving the file does NOT re-emit the editor's own inotify event back to itself.
6. External edit on a clean file is silently reloaded; on a dirty file, the banner appears with Reload/Ignore.
7. Quit with dirty state shows the dialog and only exits after the user's choice.
8. Corrupt KDL swaps to `GtkSourceView` mode; fix + resave restores GUI mode.
9. All v1 in-scope features pass `cargo test` and `cargo clippy --all-targets -- -D warnings`.
10. Manual pre-release checklist completes without surprises.

## Architecture Overview

### Naming & packaging

- Binary name: **`dotcfg-gui`** (rename from `niricfg_gui` on day zero).
- Single Cargo binary crate (workspace split deferred to v2).
- Flatpak permissions: `--filesystem=xdg-config/niri:create`; validators invoked via `flatpak-spawn --host …`.

### Module Monolith + `ToolPlugin` trait

- **Each tool owns its own AST.** Core never sees `kdl::KdlDocument`, TOML, or any format-specific type. Core speaks only `Box<dyn UndoCommand>`.
- Trait contract in `src/core/tool_plugin.rs`:

  ```rust
  pub trait ToolPlugin: Send + Sync {
      fn id(&self) -> &'static str;
      fn display_name(&self) -> &'static str;
      fn config_paths(&self) -> Vec<PathBuf>;
      fn detect(&self, path: &Path) -> bool;
      fn create_shell_page(&self) -> gtk::Widget;
      fn load(&self, path: &Path) -> Result<(), Error>;
      fn save(&self) -> Result<(), Error>;
      fn validate(&self) -> Result<Vec<ValidationIssue>, Error>;
      fn apply_saved(&self) -> Result<(), Error>;
      fn on_external_change(&self) -> ExternalChangeAction;
  }
  ```

  `ValidationIssue { line: usize, severity: Severity, message: String }`. `ExternalChangeAction::Reload | Ignore | AskUser`.

### Source layout

```
dotcfg-gui/
  Cargo.toml
  src/
    main.rs
    core/
      app_shell.rs, tool_plugin.rs, tool_registry.rs,
      file_watcher.rs, undo_stack.rs, toast.rs, state.rs
    tools/niri/
      mod.rs, config.rs, shell_page.rs, validation.rs, kdl_helpers.rs
      sections/{input, output, layout, workspaces, binds,
                gestures, animations, window_rules, layer_rules, startup}.rs
```

### App-shell behavior

- `Adw.ApplicationWindow` + `Adw.NavigationSplitView` (sidebar ↔ content). Auto-collapse on narrow windows (mirrors GNOME Settings).
- Sidebar = `gtk::ListBox` of `Adw.ActionRow` (icon, title, subtitle, dirty bullet).
- Main pane swaps in `create_shell_page()` for the active tool.
- `Adw.HeaderBar`: Undo / Redo (bound to `core::undo_stack`) + Save (sensitivity bound to dirty).
- State persistence: `glib::KeyFile` at `~/.config/dotcfg-gui/state.ini`.

### SectionContext

```rust
pub struct SectionContext {
    pub cmd_tx:   mpsc::Sender<Box<dyn UndoCommand>>,
    pub toast_tx: glib::Sender<String>,
    pub state:    glib::WeakRef<ShellState>,
}
```

### Per-section widget primitives (libadwaita)

`Adw.SpinRow` numerics; `Adw.EntryRow` strings; `Adw.ComboRow` enums; `Adw.SwitchRow` booleans; `Adw.ActionRow` sub-views; `Adw.ExpanderRow` nested. Key-chord capture in `binds.rs` via modal `Adw.Dialog` + `gtk::EventControllerKey` (no inline capture). Search on list-heavy sections (binds, window_rules, workspaces) via `Adw.ToolbarView` + `gtk::SearchBar` + `gtk::FilterListModel`.

### Data flow & threading

- UI thread: GTK + libadwaita only.
- Async runtime: `async-std` + `async-process`.
- Cross-thread bridges: `glib::MainContext::default().spawn_local` + `glib::Sender`/`Receiver`.
- Validation debounce: 250ms. Abort previous `JoinHandle` on new edit.
- Inotify backfeed: `Arc<AtomicBool> is_saving` true for 250ms around save.
- 500ms external-change debounce; mtime-on-focus-in fallback when inotify limit hit.

### Round-trip safety

KDL edits operate on untyped `kdl::KdlDocument` (never `serde`-derived structs). Edits reference nodes via **Semantic Paths** like `["binds", "Mod+Return"]`. At load time precompute `HashMap<SemanticPath, kdl::Span>`. Atomic save: `<path>.tmp` → `rename` into place.

### Error handling matrix

| Subsystem | Failure | Visible | Recovery |
|---|---|---|---|
| KDL load | Parse error | `GtkSourceView` with span highlighted | Resave → re-parse → GUI restored |
| KDL load | File missing | `Adw.StatusPage` "No Niri config" | Pristine state + Generate Default |
| Validation | `niri` not on PATH | Toast "Validation disabled" | Skip subprocess; GUI usable |
| Validation | Subprocess crash / OOM | Toast with truncated stderr | Silent restart on next edit |
| Save | Disk full / perm denied | `Adw.AlertDialog` w/ IO detail | Abort save; keep dirty |
| Save | File read-only | Persistent `Adw.Banner` "Read-Only Mode" | Lock inputs; disable Save |
| Watcher | External edit + dirty | `Adw.Banner (Reload / Ignore)` | Pause auto-reload; await choice |
| Watcher | External edit + clean | "Reloaded from disk" toast | Widget rebuild |
| Watcher | inotify limit hit | Toast warning | mtime-on-focus-in fallback |
| Shutdown | Dirty state | `Adw.AlertDialog (Save / Discard / Cancel)` | Intercept `close-request`; await response |

### Testing (xvfb + proptest)

- **Unit (no GTK)**: cargo test in CI. Commands, validator trait impls, KDL edits, undo semantics.
- **Integration**: `xvfb-run -a cargo nextest run --features gtk`.
- **End-to-end**: `tempfile::TempDir` + real `notify::RecommendedWatcher` + real `async_process::Child`.
- **Validator trait**: `trait Validator { async fn validate(kdl: &str) -> Result<Vec<ValidationIssue>, Error>; }`; test impl invokes `tests/mocks/validate-fake.sh`.
- **Property tests (proptest)**: KDL exact-string round-trip; undo commutativity over `PROPTEST_CASES=2000`; Semantic-path stability under arbitrary sibling reorder.
- **Fixture corpus** (`tests/fixtures/`): `minimal.kdl`, `typical.kdl`, `comments-rich.kdl`, `binds-heavy.kdl`, `invalid-syntax.kdl`, `nested.kdl`, `empty.kdl`.
- **CI**: `xvfb-run cargo nextest`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo audit`, `cargo +nightly fuzz run kdl_parser`.
- **Manual checklist** before release: real `niri msg validate` highlighting; Ctrl+S → real niri hot-reload; vim-side edit triggers `Adw.Banner`; malformed KDL swaps to `GtkSourceView` then restores; pristine system shows `Adw.StatusPage`; dirty close traps `close-request`.

## Implementation Process

22 ordered steps grouped into 7 DAG waves. Risk tiers: **P0** Critical Path / **P1** Likely-to-iterate / **P2** Polish. Verification: **Static** (clippy/fmt/check) / **Unit** (cargo test) / **Int** (xvfb nextest + proptest + notify + async-process) / **UI** (visual/manual).

### Wave 0 — Setup & Groundwork (sequential: 1 → 2 → 3)

1. **Cargo Init & Deps** — `cargo init --name dotcfg-gui`; `Cargo.toml` declares `gtk4`, `libadwaita` (≥1.5), `kdl`, `async-std`, `async-process`, `notify`, `proptest`, `tempfile`, `glib`, `gio`, `thiserror`. [P0, Static]
2. **Core Types** — `Error` enum (thiserror), `ValidationIssue { line, severity, message }`, `Severity` enum, `ExternalChangeAction` enum, `SectionContext` with `cmd_tx`, `toast_tx`, `state: glib::WeakRef`. [P0, Static]
3. **App State Skeleton** — `ShellState` GObject subclass (`glib::wrapper!`+`subclass`) tracking dirty flag + last validation result + current tool_id. [P1, Unit]

### Wave 1 — Architecture Core (parallel after Wave 0)

4. **Plugin Trait & Registry** — `pub trait ToolPlugin` (10 methods) in `src/core/tool_plugin.rs`; `ToolRegistry` holds `Vec<Box<dyn ToolPlugin>>`; `register`, `discover_by_path`, `sidebar_rows()` methods. [P0, Static]
5. **Undo Command Pattern** — `pub trait UndoCommand: Send { fn apply(&self, ctx); fn undo(&self, ctx); }`; `UndoStack` holds `Vec<Box<dyn UndoCommand>>` + redo buffer + dirty tracking. Header buttons bound. [P1, Unit]
6. **Shell Scaffolding** — `AdwApplicationWindow` + `Adw.NavigationSplitView` (sidebar/main); placeholder sidebar `gtk::ListBox` with `Adw.ActionRow`s per registered tool; main pane placeholder; `Adw.HeaderBar` w/ Undo/Redo/Save buttons (sensitivity no-ops v1). [P0, UI]

### Wave 2 — Niri Plugin Foundation (sequential: 7 → 8 → 9)

7. **KDL AST Wrapper** — `NiriTool` struct holds `KdlDocument` (untyped) + `load`/`save`. `niri::kdl_helpers` initial helpers (cursor traversal via `kdl::Node::children()`). `NiriTool` registered in `main.rs`. [P0, Unit]
8. **Semantic Path Indexing** — At load time, walk `KdlDocument` and precompute `HashMap<SemanticPath, kdl::Span>` for well-known paths (`["binds", "Mod+Return"]`-style). [P1, Unit]
9. **Async Validator Loop** — `fn run_validate(doc, tx)` task: 250ms sleep debounce; abort-on-new-edit via cancellable `JoinHandle`. Spawns `flatpak-spawn --host niri msg validate` (or `niri msg validate` outside Flatpak). Regex-parses stderr → `Vec<ValidationIssue>`. [P1, Int]

### Wave 3 — Section Widgets (highly parallel sub-agents)

10. **Sections Part 1 (Basic Layouts)** — `sections/input.rs` (`Adw.SpinRow` keyboard repeat, tap-to-click), `sections/output.rs` (per-monitor `Adw.ExpanderRow`). [P2, UI]
11. **Sections Part 2 (Lists & Toggles)** — `sections/workspaces.rs` (list view), `sections/layout.rs` (`Adw.SpinRow` + `Adw.ComboRow`), `sections/layer_rules.rs`. [P2, UI]
12. **Sections Part 3 (Misc Nodes)** — `sections/animations.rs`, `sections/gestures.rs`, `sections/startup.rs`. [P2, UI]

### Wave 4 — Advanced UI & Complex Sections (parallel after Wave 2)

13. **Key-Chord Modal** — In `sections/binds.rs`: "Record" button opens modal `Adw.Dialog` with `gtk::EventControllerKey`. Captures modifier + keyval, returns `Mod+Key`-style string, updates row. [P1, UI]
14. **List Search & Filtering** — Wrap `binds`, `window_rules`, `workspaces` in `Adw.ToolbarView` + `gtk::SearchBar` + `gtk::FilterListModel` over `gio::ListStore` with `gtk::StringFilter`. [P1, UI]
15. **Parser-Fallback UI** — On `kdl::parse_file` failure, swap central pane to `GtkSourceView` (libgtksourceview-5). Span highlighted via `TextTag`. Resave monitored; on success, restore GUI mode. [P1, UI]

### Wave 5 — Filesystem, State, and Safety Intercepts (parallel after Wave 1)

16. **Inotify & External Edits** — `core::file_watcher.rs`: spawn `notify::RecommendedWatcher` per active tool's `config_paths`. Channel bridges watcher events to UI. 250ms `is_saving` guard + 500ms external-change debounce + `Adw.Banner (Reload / Ignore)` on conflict. [P1, Int]
17. **Dirty Shutdown Intercept** — `Adw.ApplicationWindow::close-request`: if `state.is_dirty()`, return `Propagation::Stop`, spawn `Adw.AlertDialog (Save / Discard / Cancel)`. Apply choice, then `window.destroy()`. [P1, UI]
18. **First-Run & Settings UX** — `Adw.StatusPage` for no-config; "Generate Default" writes baseline. `glib::KeyFile` load (last tool, last file, window geometry) + save on quit. [P2, UI]

### Wave 6 — Test Automation & Packaging (parallel after Waves 2–5)

19. **Proptest Suite** — `proptest!` blocks for: KDL exact-string round-trip; undo commutativity (`apply(A,B); undo(B,A) == original`) over `PROPTEST_CASES=2000`; Semantic-path stability under sibling reorder. [P0, Int]
20. **Fixture & Mock Integration** — `tests/fixtures/{minimal,typical,comments-rich,binds-heavy,invalid-syntax,nested,empty}.kdl`; `tests/mocks/validate-fake.sh` (canned issues keyed on input); `Validator` trait + impls (mock wired in tests); `xvfb-run` runner script. [P0, Int]
21. **CI Pipeline** — `.github/workflows/ci.yml` running `xvfb-run -a cargo nextest`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `cargo audit`, `PROPTEST_CASES=2000`, `cargo +nightly fuzz run kdl_parser`. [P0, Int]
22. **Flatpak Packaging** — `com.github.<user>.dotcfg-gui.yaml`, `--filesystem=xdg-config/niri:create`, `flatpak-spawn --host` access for niri, `.desktop` entry, app icon. [P0, Manual]

## Parallelization

### DAG waves at a glance

```
Wave 0 (seq)  : 1 → 2 → 3
Wave 1 (par)  : [4 ‖ 5 ‖ 6]              ← depends on 1, 2, 3
Wave 2 (seq)  : 7 → 8 → 9                 ← depends on 4
Wave 3 (par)  : [10 ‖ 11 ‖ 12]            ← depends on 7, 8, 9
Wave 4 (par)  : [13 ‖ 14 ‖ 15]            ← depends on 7, 8, 9
Wave 5 (par)  : [16 ‖ 17 ‖ 18]            ← depends on 6
Wave 6 (par)  : [19 ‖ 20 ‖ 21 ‖ 22]       ← depends on Waves 2, 3, 4, 5
```

### Critical path

`1 → 2 → 3 → 4 → 7 → 8 → 9` is the longest dependency chain (~9 sequential steps). After Wave 1, **three independent streams** fan out (Wave 3 + Wave 4 + Wave 5), enabling parallel sub-agent execution.

### Maximum parallelization depth

Up to **4 concurrent sub-agents** during Wave 6 (proptest + fixtures + CI + Flatpak). Earlier waves permit 3 concurrent agents (Wave 1, Wave 3, Wave 4, Wave 5).

### Recommended sub-agent assignment rule

- **opus** (deep reasoning) for steps touching GObject subclassing (3, 4), cross-crate trait ergonomics (5), KDL edge cases (7, 8).
- **sonnet** (fast, high-volume) for section widget files (10–12), CI YAML (21), test fixtures (20).
- **opus** for the GUI interaction-heavy steps: key-chord modal (13), filter list model (14), parser-fallback (15), dialog intercept (17), first-run UX (18).

## Verifications

### Per-step verification levels

| # | Step | Risk | Verification |
|---|---|---|---|
| 1 | Cargo Init & Deps | P0 | Static: `cargo check`, `cargo clippy` |
| 2 | Core Types | P0 | Static: `cargo clippy`, type-level review |
| 3 | App State Skeleton | P1 | Unit: subclass compiles; dirty bit flips on signal |
| 4 | Plugin Trait & Registry | P0 | Static: trait compiles; registry insertion tests |
| 5 | Undo Command Pattern | P1 | Unit: proptest over `apply()`/`undo()` commutativity |
| 6 | Shell Scaffolding | P0 | UI: visual boot — empty AppWindow + sidebar rows render |
| 7 | KDL AST Wrapper | P0 | Unit: proptest — `serialize ∘ parse == identity` on fixtures |
| 8 | Semantic Path Indexing | P1 | Unit: index build deterministic; lookup finds nodes |
| 9 | Async Validator Loop | P1 | Int: subprocess retries after abort; debounce timing |
| 10 | Sections Part 1 | P2 | UI + Int: render widgets; round-trip preserves values |
| 11 | Sections Part 2 | P2 | UI + Int: render widgets; round-trip preserves values |
| 12 | Sections Part 3 | P2 | UI + Int: render widgets; round-trip preserves values |
| 13 | Key-Chord Modal | P1 | UI: synthetic key event in xvfb produces chord capture |
| 14 | List Search & Filtering | P1 | UI: filter narrows FilterListModel on substring query |
| 15 | Parser-Fallback UI | P1 | UI: corrupt file swaps pane; resave restores GUI |
| 16 | Inotify & External Edits | P1 | Int: tempfile + notify round-trip; banner appears on conflict |
| 17 | Dirty Shutdown Intercept | P1 | UI: synthetic close w/ dirty state shows dialog |
| 18 | First-Run & Settings UX | P2 | UI: missing config → StatusPage; KeyFile load/save round-trip |
| 19 | Proptest Suite | P0 | Int: `PROPTEST_CASES=2000` green on round-trip + undo |
| 20 | Fixture & Mock Integration | P0 | Int: each fixture parses; mock validator returns canned issues |
| 21 | CI Pipeline | P0 | Int: workflow yaml syntactically valid; runs locally |
| 22 | Flatpak Packaging | P0 | Manual: build + install Flatpak; basic flow works in sandbox |

### Test cases to cover (per acceptance criterion)

| AC# | Test cases |
|---|---|
| AC1 | - App boots from `cargo run`; window appears in xvfb screenshot. |
| AC2 | - Parsing `typical.kdl` populates sidebar; sections list visible. |
| AC3 | - With config absent, StatusPage with "Generate Default" renders; click writes baseline file. |
| AC4 | - Time edit → validation toast, measured ≤500ms wall-clock. |
| AC5 | - Save() does not re-trigger `on_external_change` (within 250ms `is_saving` window). |
| AC6 | - Modify file outside GUI while dirty → `Adw.Banner` with Reload / Ignore appears; clean auto-reload with toast. |
| AC7 | - Close with dirty → dialog traps; "Discard" then `close-request` propagates. |
| AC8 | - Inject `invalid-syntax.kdl` → GUI swaps to GtkSourceView; correct it; resave swaps back. |
| AC9 | - `cargo test` + `cargo clippy --all-targets -- -D warnings` exit 0. |
| AC10 | - Manual checklist: real `niri msg validate` highlights rows; real niri hot-reload fires on Save; vim edit triggers banner. |

### Quality gates

- **Threshold**: 3.5/5.0 (per plan-task default).
- **P0 steps must pass** Static verification before downstream steps begin.
- **P1 steps pass** at least one of Unit / Int verification before downstream fan-out.
- **P2 steps verify** visually or via test fixtures; cosmetic regressions acceptable post-v1.

### Verification summary

| Wave | Steps | Aggregate verification |
|---|---|---|
| Wave 0 | 1–3 | Static + Unit (subclass) |
| Wave 1 | 4–6 | Static + Unit + UI boot |
| Wave 2 | 7–9 | Unit + proptest + subprocess int |
| Wave 3 | 10–12 | UI + round-trip proptest |
| Wave 4 | 13–15 | UI + xvfb interaction |
| Wave 5 | 16–18 | Int (notify) + UI (dialog/statuspage) |
| Wave 6 | 19–22 | Full CI matrix + manual checklist |

## Locked-in v1 decisions

| Decision | Choice |
|---|---|
| Binary name | `dotcfg-gui` |
| Architecture | Module Monolith + `ToolPlugin` trait |
| Async runtime | `async-std` + `async-process` |
| Validation debounce | 250 ms |
| Parser-fallback UI | `GtkSourceView` |
| Save-snapshot-on-quit | YAGNI |
| Section-list search | Yes (binds, window_rules, workspaces) |
| State persistence | `glib::KeyFile` (`~/.config/dotcfg-gui/state.ini`) |
| Data binding | Semantic paths |
| Validator mechanism | `niri msg validate` via `flatpak-spawn --host` |
