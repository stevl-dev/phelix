# Helix fork: a key table per picker

Snapshot: `main` at `677dcf7a5`.
Goes in `fork/picker-tables.md`. Feature slug: `picker-tables`, part of `picker-keys`'s entry in `FORK.md`, with the commit picker's keys in `review-stream`'s.

Status: in progress. Phases 0–2 are committed.

## Bottom line

- **Build:** `[keys.picker.<name>]`, a table of keys that work in one picker only, over `[keys.picker]`'s keys for every picker. Then two actions in the commit picker that yank the selected commit's hash.
- **Why a table per picker:** `[keys.picker]` is one table for every picker. A key bound to one picker's action, like `review_jump`, is dead in the others: they pass it to the prompt. So no two pickers can give one key different actions.
- **The changed-file picker moves to its table.** `cycle_changes` (`Ctrl-g`) and `review_jump` (`Alt-r`) go to `[keys.picker.changed-files]`, and the shared table keeps the keys every picker answers. A config that binds them in `[keys.picker]` gets `unknown picker command`.
- **An action can keep the picker open.** Today every feature action closes it, and `cycle_changes` has a hook of its own to stay. One hook does both after this.
- **Biggest risk:** the config. `[keys.picker]` takes a table as a value for the first time, the global and the workspace config merge per picker, and `:set` must keep it all. Phase 1 has it.
- **Size:** 4 commits. About 250 lines in the core and 80 in `helix-review`, plus tests. This is an estimate from reading the code.

## 1. The config

```toml
[keys.picker]
C-k = "previous"
C-j = "next"

[keys.picker.changed-files]
C-g = "no_op"
A-g = "cycle_changes"

[keys.picker.review-log]
C-y = "yank_hash"
A-y = "yank_hash_to_clipboard"
```

- A string value in `[keys.picker]` binds a key in every picker, as today. A table value is a picker's own table, by the picker's name.
- A picker's table takes the commands every picker has, like `next`, and that picker's actions. Another picker's action there is `unknown picker command`, and an unknown name is `unknown picker 'x'`.
- In a picker, its own table's binding goes first, then the shared table's.
- `no_op` in a picker's table hands that key to the prompt in that picker, even if the shared table binds it. In `[keys.picker]` it drops the key, as today.
- The workspace config's tables go over the global config's, key by key, the shared table and each picker's alike.

| Picker | Name | Its actions |
| --- | --- | --- |
| Changed files (`Space g`) | `changed-files` | `cycle_changes` (`Ctrl-g`), `review_jump` (`Alt-r`) |
| Commits (`:review-log`) | `review-log` | `yank_hash`, `yank_hash_to_clipboard` (`Ctrl-y`) |

Every other picker has no name and no table, and reads the shared table alone.

## 2. Design

### 2.1 The tables (phase 1)

- `PickerKeys` in `helix-view/src/editor/picker_keys.rs` holds the shared table and the tables by picker name, with one lookup: a picker's name, if it has one, and a key. `Config::picker_keys` holds it.
- `Picker::with_keys(name)` names a picker's table. The changed-file picker is `changed-files`.
- `PickerCommand::CycleChanges` goes. `cycle_changes` is an action of the changed-file picker, `PickerCommand::Action("cycle_changes")`, which still runs through `on_cycle_changes` in this phase.
- The pass-through for an action a picker doesn't answer goes: a picker reads only the shared table and its own.
- `helix-term/src/feature.rs` gets the names: the core declares `changed-files` and `cycle_changes`, and a feature's changed-file actions join that table. `Feature::picker_actions` becomes `changed_file_actions`, as that's all it ever held.
- `config.rs` parses the nested tables and merges them, and `Application` keeps them across `:set`, as it keeps the shared table today.

### 2.2 An action that stays (phase 2)

- `with_on_action`'s callback gets the picker, so it can read the selection or change the items, and returns whether the picker closes.
- `cycle_changes` runs through it, and `on_cycle_changes` goes.
- `review_jump` closes the picker, as today.

### 2.3 A feature's own picker (phase 3)

- `Feature` gets `pickers`: each picker's name and its actions, with each one's default key, if it has one.
- The commit picker is `review-log`, with `yank_hash` and `yank_hash_to_clipboard` on `Ctrl-y`.
- `yank_hash` writes the selected commit's full hash to `default-yank-register`, and `yank_hash_to_clipboard` to `+`, as `yank` and `yank_to_clipboard` write the selection.
- The picker stays open. The status line says `yanked <full hash> to register +`, after `yank`'s `yanked 1 selection to register +`. A register that refuses the write, like a clipboard that fails, says why as an error.

## 3. Left out

- Tables for the other pickers, like the file picker. Each can get a name when it has an action of its own.
- A yank of the subject, or of the hash and subject together.
- Yanking from other pickers, like a file's path from the file picker.
- `[keys.picker]` in the command palette or `:keymap` listings.

## Plan

Each phase is one commit that builds and passes its tests, and marks itself done here.

| # | Status | Commit | Work |
| --- | --- | --- | --- |
| 0 | Done | Add the plan for a key table per picker. | This file. |
| 1 | Done | Give a picker a key table of its own. | 2.1. Unit tests in `config.rs`, from TOML: a picker's binding beats the shared one, a key it lacks falls back to the shared one, `no_op` in its table hands the key to the prompt there and nowhere else, a workspace table goes over the global one key by key, an action in another picker's table and an unknown picker are refused with their messages. `review_jump`'s test in `helix-review/src/term.rs` moves to `[keys.picker.changed-files]`. The `:set` test in `helix-term/tests/test/commands.rs` keeps a picker's table too. Book (`keymap.md`) and `FORK.md`. |
| 2 | Done | Let a picker action keep the picker open. | 2.2. No behavior changes. The changed-file picker has no end-to-end test, since it walks the working directory, so `cycle_changes` has none. Phase 3's tests show an action that stays. |
| 3 | | Yank a commit's hash from the commit picker. | 2.3. Integration tests in `helix-review/tests/test/review/commit.rs`, on `clipboard-provider = "none"`, so no test writes your clipboard: `Ctrl-y` puts the selected commit's full hash in `+` and the picker stays on screen; `yank_hash` bound in `[keys.picker.review-log]` puts it in `"`; `Ctrl-y` in another picker, like the buffer picker, types into its prompt. Book (`keymap.md`), generated docs if they list the actions, and `FORK.md`. |

Phase 1 comes first because it has the risk, and the others need its tables.
Phase 2 comes before 3, since the yank needs an action that stays.

After phase 1, check in `phx`: `Ctrl-g` and `Alt-r` in the changed-file picker, and a binding in `[keys.picker.changed-files]`. After phase 3: `Ctrl-y` in `:review-log`, then paste outside the editor.

## What the work found

- **Phase 1.** `PickerKeys` in `picker_keys.rs` holds the tables, with `get` and `overlay`, and replaces `overlay_picker_keys`. `feature::picker_keys` lists every table's actions: the core's `cycle_changes`, then the features' changed-file actions. A picker table's name is valid when some action names it.
  The changed-file picker's name and `cycle_changes` are constants in `changed_file.rs`. Until phase 2, the picker's key handler names `cycle_changes` to run it through `on_cycle_changes`, the one place outside `changed_file.rs` that does.
  A picker with an action bound but no `on_action` now ignores the key, where it handed it to the prompt. No picker has one: an action binds only in its own picker's table.
- **Phase 2.** `with_on_action`'s callback takes the picker, the context, and the action's name, and returns `AfterAction`, `Close` or `Stay`. The picker no longer reads the selection for it: `cycle_changes` runs with none, and the changed-file picker's other actions stay open without one, as they did.
  The changed-file picker's lists stay locked through a cycle, as before, and not through a feature's action, since the scan's thread records each change under that lock.
