# dotcfg-gui — Pre-Release Checklist (AC10)

> Run every item on a **real Wayland session with niri running** unless
> noted otherwise. Mark each item ✅ PASS / ❌ FAIL / ⏭ SKIP. Do not
> release with any ❌ items unresolved.

---

## 1. Build & CI

- [ ] **`cargo build --features gtk`** exits 0
- [ ] **`cargo test --no-default-features`** passes (all 11 integration tests)
- [ ] **`cargo clippy --all-targets --features gtk -- -D warnings`** exits 0
- [ ] **`cargo fmt --all -- --check`** exits 0
- [ ] **CI pipeline** — all 5 jobs (Audit, Lint, Tests Debug, Property Tests Release, Fuzz) show `completed-success`

---

## 2. First-Run UX

*Prerequisites: move or rename `~/.config/niri/config.kdl` so no config exists.*

- [ ] **StatusPage renders** — `cargo run --features gtk` shows `Adw.StatusPage` with "Welcome to dotcfg-gui" and a "Generate Default Config" button
- [ ] **Generate writes baseline** — click the button; verify `~/.config/niri/config.kdl` exists with valid KDL content
- [ ] **Editor opens after generation** — after clicking Generate, the window transitions from StatusPage to the editor with sections, raw, and diff tabs visible
- [ ] **File watcher starts after generation** — after Generate, externally modify the file (e.g. `echo >> ~/.config/niri/config.kdl`), and verify the editor detects the change within ~1s
- [ ] **Window geometry persisted** — resize the window, close with `Ctrl+Q`, reopen; verify window size matches the previous session

---

## 3. Opening Existing Config

*Prerequisites: `~/.config/niri/config.kdl` exists with valid content.*

- [ ] **Sidebar shows Niri tool** — sidebar `GtkListBox` has an item labelled "Niri"
- [ ] **Sections tab renders** — main pane shows the "Sections" tab with Input, Output, Layout, Workspaces, Layer Rules, Binds, Animations, Gestures, Startup groups
- [ ] **Raw tab shows KDL** — the "Raw" tab shows the full config text with monospace font and KDL syntax highlighting applied
- [ ] **Tab switching via sidebar** — clicking different sidebar rows switches the active tab
- [ ] **Window title** — window title reads `dotcfg-gui — niri config editor`

---

## 4. Editing & Validation

- [ ] **Validation runs on edit** — type an invalid value (e.g. `touchpad { tap-to-click bogus }`), wait ~500ms; verify the `Adw.Banner` appears with a validation error from `niri validate --config <tmpfile>`
- [ ] **Validation clears on fix** — correct the invalid value, wait ~500ms; verify the banner hides
- [ ] **Validator writes tempfile** — while editing, check `/tmp/` for a `.tmp` file being created and immediately cleaned up (use `inotifywait -m /tmp` in a second terminal)
- [ ] **Validation without niri installed** — temporarily rename the `niri` binary (`mv $(which niri) $(which niri).bak`), edit a value; verify the banner shows "niri binary not found — validation skipped" without crashing
- [ ] **Debounce timing** — rapidly type 10 characters; verify only **one** validation run triggers after ~500ms of idle (check via `strace -e execve` or terminal logs)

---

## 5. Save (Atomic Write)

- [ ] **Save button sensitivity cycle** — verify: Save button starts **disabled** → make an edit → button becomes **enabled** → click Save → button becomes **disabled** again
- [ ] **Save persists to disk** — edit a value, click Save (or `Ctrl+S` not yet wired — use Save button), close the app, reopen; verify the edit is present
- [ ] **Atomic write preserves structure** — Save, then inspect the file: verify `~/.config/niri/config.kdl` is a regular file (not a temp file) and content matches the editor
- [ ] **No spurious inotify event** — save a file while watching with `inotifywait -m ~/.config/niri/config.kdl`; verify the save doesn't trigger a "Reloaded from disk" banner
- [ ] **Save with broken KDL** — intentionally break the KDL syntax, click Save; verify the file is saved (fallback to direct atomic write), then fix the syntax and save again (restores comment-preserving path)
- [ ] **Save to read-only file** — `chmod -w ~/.config/niri/config.kdl`, edit, try to Save; verify the app doesn't crash (dialog or silent failure expected)
- [ ] **Multiple rapid saves** — make an edit, click Save 5 times rapidly; verify only one save effect on disk

---

## 6. External Edits (File Watcher)

*Prerequisites: editor open with unsaved changes.*

- [ ] **Clean file, external edit → silent reload** — make the config clean (no dirty edits), modify the file externally (`echo >> ~/.config/niri/config.kdl`); verify the editor silently reloads the new content
- [ ] **Dirty file, external edit → Reload/Ignore banner** — make the config dirty (edit text), modify the file externally; verify an `Adw.AlertDialog` appears with "Reload" and "Ignore" options
- [ ] **Reload discards edits** — in the dirty conflict dialog, click "Reload"; verify the editor text matches the on-disk file and the dirty flag is cleared
- [ ] **Ignore keeps edits** — in the dirty conflict dialog, click "Ignore"; verify the editor keeps the unsaved text
- [ ] **500ms external-change debounce** — rapidly write to the file 3 times with `for i in 1 2 3; do echo $i >> ~/.config/niri/config.kdl; done`; verify only **one** reload/dialog fires

---

## 7. Dirty Shutdown Intercept

- [ ] **Clean close exits immediately** — with no unsaved changes, press `Ctrl+Q` or close the window; verify the window closes **without** a dialog
- [ ] **Dirty close shows Save/Discard/Cancel** — make an edit, close the window; verify `Adw.AlertDialog` with "Save", "Discard", "Cancel" appears
- [ ] **Save in shutdown dialog** — make an edit, close, click "Save" in the dialog; verify the file is saved on disk, then the window closes
- [ ] **Discard in shutdown dialog** — make an edit, close, click "Discard"; verify the file is **not** updated, the window closes
- [ ] **Cancel in shutdown dialog** — make an edit, close, click "Cancel" (or press Escape); verify the window stays open with edits intact
- [ ] **Window state persisted on close** — resize the window, close (clean), reopen; verify geometry is restored

---

## 8. Corrupt KDL → Fallback → Restore

- [ ] **Invalid KDL swaps to fallback view** — paste invalid KDL (e.g. `input {`) into the editor; verify the stack switches to the fallback view with the error span highlighted in red
- [ ] **Error message shown** — in the fallback view, verify a descriptive error message is visible above the text editor
- [ ] **Fix + Restore GUI** — correct the KDL, click "Restore GUI"; verify the stack switches back to the Sections/Raw view
- [ ] **Restore GUI with still-broken KDL** — click "Restore GUI" while the KDL is still broken; verify the error display updates with the new diagnostic, and the view stays in fallback mode
- [ ] **Sections widget rebuilt after Restore** — after restoring, verify the Sections tab shows the correct structured editors (SpinRows, Switches, etc.) matching the fixed KDL

---

## 9. Undo / Redo

- [ ] **Undo button disabled on startup** — verify the undo button (`edit-undo-symbolic`) is disabled/greyed when the editor has no history
- [ ] **Redo button disabled on startup** — verify the redo button (`edit-redo-symbolic`) is disabled/greyed
- [ ] **Undo restores previous text** — type "hello world" in the editor, press Undo; verify the text reverts to the state before the keystroke
- [ ] **Redo restores undone text** — after undoing, press Redo; verify the text returns to "hello world"
- [ ] **Undo from empty state** — type a single character on a blank buffer, press Undo; verify the buffer returns to empty
- [ ] **No re-entrancy corruption** — Undo, then immediately type more text; verify the undo stack still works for the new edits
- [ ] **Multi-step undo chain** — type "a", "b", "c" sequentially; undo 3 times; verify the buffer goes "abc" → "ab" → "a" → ""
- [ ] **Multi-step redo chain** — after 3 undos, redo 3 times; verify the buffer goes "" → "a" → "ab" → "abc"

---

## 10. Syntax Highlighting

- [ ] **Keywords colored** — verify KDL keywords like `binds`, `input`, `output`, `layout`, `spawn-at-startup` are highlighted in a distinct color
- [ ] **Strings colored** — verify quoted strings like `"foot"`, `"#5294e2"` are colored differently from keywords
- [ ] **Comments colored** — verify `//` and `/* */` comments are styled distinctly (e.g. grey/italic)
- [ ] **Numbers colored** — verify numeric values like `250`, `33`, `8` are highlighted
- [ ] **Highlighting updates on edit** — modify the text; verify the highlighting re-applies correctly to the new content

---

## 11. Diff View

- [ ] **Compare toggle exists** — verify the "Compare" toggle button in the sidebar HeaderBar
- [ ] **Diff shows changes** — make an edit, click "Compare"; verify the side-by-side diff shows the original vs modified text with colour-coded additions/removals
- [ ] **Line numbers on both sides** — verify the left (original) and right (modified) panes each have line-number gutters
- [ ] **Scroll sync** — scroll in one diff pane; verify the other pane scrolls in sync
- [ ] **Toggle off restores editor** — click "Compare" again to deactivate; verify the view returns to the editor
- [ ] **Diff persists across tab switches** — activate Compare on tab 1, switch to tab 2, switch back; verify tab 1 still shows the diff view

---

## 12. Search & Filter (binds / workspaces / window_rules)

- [ ] **SearchBar appears** — in the binds section, verify a search bar is present (or appears when typing)
- [ ] **Filter narrows list** — type a substring (e.g. "Mod") in the search bar; verify only matching items remain visible
- [ ] **Filter clears** — clear the search bar; verify all items return
- [ ] **Search is case-insensitive** — type "mod" and "Mod"; verify the same results appear

---

## 13. Key-Chord Modal (binds section)

- [ ] **Record button present** — verify each bind row has a "Record" button
- [ ] **Modal opens** — click "Record"; verify an `Adw.Dialog` appears
- [ ] **Key capture works** — with the modal open, press `Mod+Shift+A`; verify the chord string "Mod+Shift+A" is captured and the dialog closes
- [ ] **Row updates** — after the modal closes, verify the bind row shows the captured chord
- [ ] **Escape cancels** — open the modal, press Escape; verify the modal closes without modifying the row

---

## 14. Side-by-Side Diff + Compare Toggle

- [ ] **Compare toggle in sidebar HeaderBar** — verify the `Compare` toggle button is in the sidebar header, after the Save button
- [ ] **Toggling Compare updates all tabs** — open 2 config tabs (if supported; currently single-tool), activate Compare; verify each tab's content switches to diff mode
- [ ] **Diff computed against saved snapshot** — make an edit, activate Compare; verify the diff shows the saved-on-disk version vs the edited version (not empty vs current)

---

## 15. Window State Persistence

- [ ] **`~/.config/dotcfg-gui/state.ini` created** — after running the app and closing it, verify the state file exists
- [ ] **Window geometry restored** — resize to 1200×800, close, reopen; verify the window opens at 1200×800
- [ ] **Last tool saved** — if multiple tools exist (v1: just Niri), verify the state file contains the last-active tool ID

---

## 16. Error Handling (Error Matrix)

- [ ] **`niri` not on PATH** — move the niri binary aside, edit the config; verify the validation banner shows a warning-level issue ("niri binary not found — validation skipped"), the app does **not** crash or freeze
- [ ] **Save to disk-full filesystem** — create an FS image, mount it, symlink the config there, fill the FS; edit and Save; verify the app shows an error dialog, does **not** crash, and the dirty flag stays true
- [ ] **Config directory missing** — `rm -rf ~/.config/niri`, start the app; verify the StatusPage shows, Generate creates the directory and file
- [ ] **Inotify limit hit** — `echo 0 | sudo tee /proc/sys/fs/inotify/max_user_watches`, start the app; verify the watcher fails gracefully (the editor still works), no crash

---

## 17. Flatpak Build (conditional — skip if not yet packaged)

- [ ] **Flatpak manifest builds** — `flatpak-builder build-dir com.github.d3t0x.dotcfg-gui.yml --force-clean` exits 0
- [ ] **Flatpak installs** — `flatpak-builder --install build-dir --user` exits 0
- [ ] **Flatpak launches** — `flatpak run com.github.d3t0x.dotcfg-gui` shows the window
- [ ] **`flatpak-spawn --host` path works** — in the Flatpak sandbox, validation calls `flatpak-spawn --host niri validate --config <tmpfile>` instead of bare `niri`
- [ ] **Filesystem access** — Flatpak has `--filesystem=xdg-config/niri:create` permission; verify Generate and Save work
- [ ] **`.desktop` entry** — verify the app appears in the application launcher with the correct name and icon

---

## 18. Regression: Headless Build

- [ ] **`cargo build --no-default-features`** exits 0 (no GTK deps)
- [ ] **`cargo test --no-default-features`** passes all tests (no xvfb needed)
- [ ] **Binary prints usage** — `cargo run --no-default-features` prints "built without gtk feature" (or similar) and exits 0

---

## Summary

| Section | Items | ✅ PASS | ❌ FAIL | ⏭ SKIP |
|---------|-------|---------|---------|---------|
| 1. Build & CI | 5 | | | |
| 2. First-Run UX | 5 | | | |
| 3. Opening Config | 5 | | | |
| 4. Edit & Validation | 5 | | | |
| 5. Save | 7 | | | |
| 6. External Edits | 5 | | | |
| 7. Shutdown | 6 | | | |
| 8. Corrupt KDL | 5 | | | |
| 9. Undo/Redo | 8 | | | |
| 10. Highlighting | 5 | | | |
| 11. Diff View | 6 | | | |
| 12. Search/Filter | 4 | | | |
| 13. Key-Chord | 5 | | | |
| 14. Compare | 3 | | | |
| 15. State Persist | 3 | | | |
| 16. Error Matrix | 4 | | | |
| 17. Flatpak | 6 | | | |
| 18. Headless | 3 | | | |
| **Total** | **83** | **/** | **/** | **/** |

**QA sign-off:** \_\_\_\_\_\_\_\_\_\_\_\_\_\_\_\_ Date: \_\_\_\_\_\_\_\_\_\_\_\_\_\_\_\_
