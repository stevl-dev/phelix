# Fork features

What this fork adds to Helix or changes in it, one entry per feature.
`AGENTS.md` has the rules for writing code here. `fork/<feature>.md` has a feature's plan.

Update a feature's entry in the same commit as the feature.

Fork point: upstream `ba40e547`, 2026-09-29.

## Branches

- `main` is the fork's line, the branch to build, and the default branch on GitHub.
- `origin/main` only fast-forwards, through `tools/fork/land.sh`. Nothing force-pushes it.
- `upstream/master` is upstream's line. `git merge-base main upstream/master` is the fork point, since cherry-picks don't change it.

## Setup

In a new clone:

```sh
git remote add upstream https://github.com/helix-editor/helix.git
git remote set-url --push upstream no_push
gh repo set-default stepan-ogorodnikov/helix
git config core.hooksPath tools/fork/hooks
```

`pre-commit` runs on commit. It formats the crates the staged Rust files belong to, and runs clippy on them.

## Build

`cargo install --path helix-term --locked` installs `hx` from the checkout.
`book/src/building-from-source.md` says how to point it at the `runtime` directory.

## Features

Each entry says what the feature adds, and where its code, tests, and book pages are.
"Relies on" names upstream code the feature copies or depends on. A cherry-pick that changes it has to check the feature (`agents/UPSTREAM.md`).

### picker-keys

- Adds: `[keys.picker]` in `config.toml`.
  The built-in picker bindings stay, entries override them, and `no_op` returns a key to the query prompt.
- Code: the table in `helix-view/src/editor/picker_keys.rs`, re-exported from `editor.rs` beside the `Config::picker_keys` field.
  `helix-term/src/config.rs` parses `[keys.picker]` and merges it over the defaults, and the key handler in `helix-term/src/ui/picker.rs` looks commands up in the table.
- Book: `book/src/keymap.md`.
- Tests: `picker_keys_overlay_defaults_and_no_op_unbinds` in `config.rs`. No integration test.

### changed-files-views

- Adds: the changed-files picker (`Space g`) lists unstaged, staged, or all changes.
  It opens on unstaged. `Ctrl-g` (`cycle_changes`) cycles the list, and the current list shows beside the match count.
- Code: the picker in `helix-term/src/commands/changed_file.rs`.
  The status walk in `helix-vcs/src/git.rs` also reports staged changes (`status`, `staged_file_change`, `absolute_path`), and `for_each_changed_file` in `helix-vcs/src/lib.rs` passes each change's `ChangeKind`.
  `helix-vcs/src/status.rs` has `ChangeKind`, `ChangeView`, and `visible_changes`. `helix-term/src/ui/picker.rs` has the status label, the `on_cycle_changes` hook, and `replace_items`.
- Book: `book/src/keymap.md`.
- Tests: `changed_files_include_staged_and_unstaged` in `helix-vcs/src/git/test.rs`, `all_keeps_one_entry_and_prefers_unstaged` in `status.rs`.
- Relies on: upstream's status walk in `git.rs` and `lib.rs`. Upstream's #9951 and #16252, both open, rewrite it, so taking either means redoing the staged walk on top.

### yank-diagnostics

- Adds: `:yank-diagnostics` copies every diagnostic in the buffer as `path:line:col: severity: message`, to the clipboard by default.
  Upstream's `:yank-diagnostic` only copies the ones under the cursor.
- Code: `helix-term/src/commands/typed/yank_diagnostics.rs`, a child of `typed.rs`, so it can call the private `yank_register`.
- Book: `book/src/generated/typable-cmd.md`, generated.
- Tests: two unit tests in `yank_diagnostics.rs`.

### align-message

- Adds: the message below the statusline is drawn one column in, under the statusline's first text.
  Upstream draws it from column 0, while the statusline's elements pad themselves with a leading space.
- Code: the indent where `render` in `helix-term/src/ui/editor.rs` draws the status message.
- Tests: `status_message_starts_under_the_statusline_text` in `helix-term/tests/test/statusline.rs` reads the drawn bottom row.
  It reaches the screen through `Application::terminal_buffer`, a test-only accessor under the `integration` feature.
- Rulings: the indent is always on, with no config key, by the human's call.
- Gaps: the command line prompt (`:`) on the same row still starts at column 0.

### fork-infra

- Plan: `fork/fork-infra.md`, and `fork/hard-fork.md` for the move to a hard fork.
- Adds: the agent rules in `AGENTS.md` and `agents/`, this list, the `pre-commit` hook, and `tools/fork/land.sh`, the only CI.
- Files: `AGENTS.md`, `FORK.md`, `fork/*.md`, `agents/`, `tools/fork/`.
- Removes: upstream's `.github/workflows/`, `.github/actions/`, `.github/dependabot.yml`, and `.github/FUNDING.yml`.
  `docs/CONTRIBUTING.md` no longer lists `build.yml` among the places that name the MSRV.

### review-stream

- Plan: `fork/review-stream.md`, done through phase 9f.
  The completion spike's prototype is on the local branch `spike/review-completion`, which is never pushed.
- Adds: `:review` shows every changed file's hunks against `HEAD` in one buffer, or with `unstaged` or `staged` against the index, with old and new line numbers and `+`/`-` signs in the gutter.
  An edit to added or unchanged lines, new and deleted lines too, goes to the file's buffer as you type, loaded in the background if need be and closed with the review once saved, and the rows follow it. New lines and indents are the file's own. Any other edit is put back, and a staged review takes none.
  `u` and `U` in the review (`review_undo` and `review_redo`, swapped in for `undo`, `redo`, `earlier`, and `later`) undo and redo its edits and restores in every file they changed, and stop at a file that changed since in another way.
  `:w` in the review writes every file it shows with unsaved changes, through upstream's save chain, and `u` then undoes what the save changed first.
  `goto_definition`, `goto_declaration`, `goto_type_definition`, `goto_implementation`, `goto_reference`, `hover`, and `code_action` in the review ask the language server of the row's file, at the same place in it.
  `rename_symbol` in the review opens the review's own prompt, and the rename is a step of the review's undo log.
  Each file's diagnostics show on its rows, from its buffer, or from what its servers reported when it isn't open.
  Completion in the review offers the items of the row's file's language servers and opens on their trigger characters. Selecting an item there doesn't preview it in the text. An item's other edits, like an import, go to the file after the item, in the same undo step. Signature help in the review asks the row's file's language server, and opens on its trigger characters.
  The review has a tab of its own (`tabs`). `Enter` in it (`review_open`) switches to the code tab and opens the row's file at that line, and `:review` comes back. On a row of unchanged lines, `Enter` shows them. A goto, a picker, or `:e` in the review's view opens in the code tab too, and a split opened on a file stays in the review's tab. `:bc` in the review closes it and its tab, and refuses while a file it shows has unsaved changes. `:bc!` closes it anyway, and drops the changes of the files it loaded. It stays out of the buffer lists, and goes by `[review]` (`tabs`).
  An edit to a file's buffer shows in the review, with the cursor kept on its line. With a review open, `:review` switches to it, and `:reload` there, or `:reload-all` from either tab, lists and reads the changes again. When that read, or a stage, finds none, the review closes and names the files with unsaved changes. `:review` with a scope that has none leaves the open review.
  In the review, `goto_next_change` and the other change motions select the review's hunks across files, on whatever keys they're bound to, and `goto_next_buffer`, `goto_previous_buffer`, `:bn`, and `:bp` go between files.
  `:stage-hunk` and `:unstage-hunk` write the hunks the selection touches to the index through the `git` binary, in a trusted workspace.
  `:review-restore` puts the base's text back over them in the files' buffers. `:review-context` sets the lines of context.
  In the changed-file picker, `review_jump` (`Alt-r`) jumps to the selected file in the review of the list's changes.
  Rows get syntax colors from parses of each file's whole base and source.
  Removed and added rows get tints, headers a rule, and changed words a mark, from theme scopes: `diff.{plus,minus}.view`, `diff.delta.view.text`, and the fork's `ui.review.header` and `ui.review.gap`.
  `[review]`'s `row-background` and `word-background`, a top-level table of `config.toml`, turn the tints and the marks off. `:config-reload` reads it again, and `:set` and `:toggle` don't reach it.
- Code: `helix-view/src/review.rs` and `review/{layout,reconcile,forward,gutter,diagnostics,source,log,sync,syntax,words,complete}.rs`, `helix-term/src/commands/review.rs` and `review/{lsp,stage,restore,write}.rs`, `helix-term/src/keymap/review.rs`, `helix-term/src/ui/text_decorations/review.rs`, and `helix-vcs/src/git/index.rs` with the `helix-vcs/src/index.rs` facade.
  It hooks in at:
  - `helix-term/src/config.rs`: the review's row in `FEATURE_TABLES`, which reads `[review]` through `commands::review::apply_config` (`feature-hooks`).
  - `helix-term/src/handlers.rs`: `setup` calls `commands::review::register`, which runs `review::sync` on `EditorWillUpdate` and registers the UI part of the review's buffer kind (`feature-hooks`).
  - `helix-term/src/ui/picker.rs`: the `on_review_jump` hook, its builder, and its key arm.
    `picker_keys.rs` (`picker-keys`) has `ReviewJump` and its `Alt-r` default, and `changed_file.rs` (`changed-files-views`) sets the hook.
- Book: `book/src/keymap.md` (Review), `book/src/themes.md` (the review's scopes), `book/src/configuration.md` (`[review]`), and `book/src/generated/static-cmd.md` and `typable-cmd.md`, generated.
- Tests: unit tests in `review.rs`, `review/layout.rs`, `review/reconcile.rs`, `review/forward.rs`, `review/syntax.rs`, `review/words.rs`, `review/complete.rs`, `helix-vcs/src/git/index.rs`, `commands/review.rs` (`[review]`), and `commands/review/stage.rs`. Integration tests in `helix-term/tests/test/review.rs` over a temporary git repository. `review/rust_analyzer.rs` runs completion and a goto against rust-analyzer. It's ignored, since it changes the working directory, and runs alone at a manual check: `cargo integration-test -- --ignored rust_analyzer --test-threads=1`.
- Relies on:
  - The review's rename in `commands/review/lsp.rs` follows `rename_symbol`'s request and edit, without `prepareRename`. A cherry-pick that changes them updates it.
  - `commands/review/write.rs` copies `write_all_impl`'s save chain. A cherry-pick that changes the chain updates the copy.
  - `imara-diff` in `helix-view/Cargo.toml` moves together with helix-core's and helix-vcs's.
- Rulings:
  - The dropped diff review feature's plan and code stay under the local tag `archive/diff-review`. The tag is never pushed.
  - Staging runs the `git` binary: `git hash-object --path` and `git update-index`, which convert text the way `git add` does. Upstream reads diffs with gix only and never needs it.
  - The stream keeps itself read-only from `render`, not from event hooks, so one check catches edits from keys, typable commands, and paste before they're drawn.
  - The stream sets `gutters.line_numbers.min_width` on the views that show it, because `GutterType::width` gets no `&Editor`.
  - The review's LSP keys run upstream's own commands, which ask the review's buffer kind for the row's file (`feature-hooks`). Only its rename is its own, swapped into its keymap and not in the command table, since it logs the rename as a step of the review's undo log.
  - `u` in the review walks the review's own log of the files' revisions, since the stream's history is cleared on every sync. When a file moved on since a step, the undo stops and names it, rather than undoing the file's own latest change.
  - A refusal over unsaved files leaves you in the review, by the human's call. Upstream switches the view to the first unsaved buffer, which in the review's view would move it to the code tab.
  - `:bc` in the review refuses while a file it shows has unsaved changes, the files `:w` there writes, by the human's call. It used to close and leave the files it loaded open and out of sight, with no message. `:bc!` drops the changes of the files it loaded, and a file you had open keeps its own.
  - `gn`, `gp`, `:bn`, and `:bp` in the review go between its files, and stop at the first and the last, by the human's call. They used to put a listed buffer in the review's view, which the sync then moved to the code tab.
    They replaced the review's own `]F` and `[F` (`review_next_file`, `review_prev_file`), which did the same.
  - A read that finds none of the review's changes closes it, by the human's call, so no changes means no review, as at `:review`. It used to leave the review open and empty, and `:review` then switched to it without reading the changes again. A file it showed with unsaved changes keeps its buffer and is named in the status line. A scope with no changes leaves the open review, which it used to empty.
  - `:reload` in the review, and `:reload-all`, read the review's changes again, by the human's call. `:reload` there did nothing, since the review's buffer has no file. They replaced `:review-refresh`, which did the same.
    Nothing reads them on its own, as when the terminal gets focus back, by the human's call for now.
  - What opens in the review's view moves to the code tab through a check in the sync, which reads what the review tab's views show, not which command ran. So it covers a goto whose answer comes after the key, the pickers, and typable commands, with no hook in each.

### tabs

- Plan: `fork/tabs.md`, done.
- Adds: tabs, each a layout of its own with its own splits and focus. Features open them, one of each kind: `code`, where Helix starts, and `review`.
  `:q`, `:q!`, and `Ctrl-w q` on a tab's last view quit Helix, from any tab. A buffer close that takes a tab's last view closes that tab and goes back to `code`. The code tab never closes, and gets a new view instead, as the editor did.
  `:review` opens the review tab (`review-stream`). `:tab <name>` switches to an open tab, and `:tab` names the active one.
  A feature's buffer can stay out of the buffer lists, as the review's does: the bufferline, `Space b`, `:buffer`'s completion, `:bn` and `:bp`, and `:bca` and `:bco`. When a buffer close empties the code tab, its new view takes a listed buffer.
  A buffer with no path can go by a name of its own in place of `[scratch]`, as the review's `[review]` does, in the statusline and `%{buffer_name}`.
- Code: `Tab` and the tabs in `helix-view/src/tree.rs`. A tab is a name, `Tab("review")`, with `Tab::CODE` the home tab. Every tab's root container is in the tree's one node map, and `root` and `focus` are the active tab's. `Tree::tabs` lists the open tabs in the order they opened, the code tab first. `Editor::open_tab`, `switch_tab`, `close_tab`, and `quit_view` in `helix-view/src/editor.rs`. A tab opened with `TabOptions { keeps: Some(kind) }` keeps that kind's buffers: `Editor::send_out_of_kept_tabs` gives the code tab what a view of it shows in place of one, and puts the buffer back.
  It hooks in at:
  - `helix-view/src/editor.rs`: `focus` switches to a hidden view's tab, `close_document` gives the code tab a view, `should_close` checks every tab, and `switch` keeps a scratch buffer that a view in another tab shows.
  - `helix-term/src/commands/typed.rs`: `quit` and `force_quit` call `quit_view`. `helix-term/src/commands.rs`: so does `wclose`, and `wonly` closes the active tab's views only.
  - `helix-term/src/ui/editor.rs`: the draw loop and the mouse's hit tests walk the active tab.
  - `:tab` is `tab` in `typed.rs`, and its completer `completers::tab` in `helix-term/src/ui/mod.rs`. `:tab` tells a name no feature opens from a tab that isn't open by `TABS`, the tabs features register (`feature-hooks`).
  - `Document::name_without_path` in `helix-view/src/document.rs`, read by `display_name`, the statusline's three file name elements in `helix-term/src/ui/statusline.rs`, and `%{buffer_name}` in `helix-view/src/expansion.rs`.
  - `Document::listed` in `helix-view/src/document.rs`, which the review clears. The lists filter on it: `render_bufferline` in `ui/editor.rs`, `buffer_picker` and `goto_buffer` in `commands.rs`, the two `buffer_gather_*_impl` in `typed.rs`, `completers::buffer` in `ui/mod.rs`, and `close_document`'s new view in `editor.rs`.
- Book: `book/src/keymap.md` (Review), `book/src/command-line.md` (`buffer_name`), and `book/src/generated/typable-cmd.md`, generated.
- Tests: unit tests in `tree.rs`, integration tests in `helix-term/tests/test/tabs.rs`.
- Relies on: `Tree::views` walks every tab's views, and `Tree::traverse` the active tab's. Upstream code that walks `views()` for what's on screen, as a cherry-pick may add, has to use `traverse()`.
  A cherry-pick that adds a list of buffers, or a sweep over them, filters on `Document::listed`.
- Rulings:
  - `:q` on a tab's last view quits Helix from any tab, by the human's call, where Vim's closes the tab.
  - The name is `tab`, by the human's call. "Space" reads as Space mode.

### feature-hooks

- Plan: `fork/feature-hooks.md`, done through phase 11a.
- Adds: general hooks the core calls where it used to check for a feature. A feature's config is a top-level table of `config.toml` of its own, and an unknown top-level table is still refused.
- Hooks:
  - `EditorWillUpdate { editor, completing }`, an event before each key and each frame. Defined in `helix-term/src/events.rs`, and dispatched at the top of `EditorView::handle_event`, before the keymap swap, and of `EditorView::render`, in `helix-term/src/ui/editor.rs`. `completing` is whether a completion menu is open.
  - `BufferKind` in `helix-view/src/kind.rs`, the answers a feature's buffer gives where the core would act on an ordinary one, and `Document::kind`, which points at one. A feature declares its kind as a `static`.
  - `BufferKind::source`, the buffer a language feature asks from. `Editor::source` returns the view's own buffer, or asks its kind. `source` in `helix-term/src/commands/lsp.rs` calls it for `goto_definition`, `goto_declaration`, `goto_type_definition`, `goto_implementation` (all through `goto_single_impl`), `goto_reference`, `hover`, and `code_action`. Their requests read the source, and their answers the focused view: the jump records where you were, and the popups open at its cursor.
    `request_completions` and `request_incomplete_completion_list` in `helix-term/src/handlers/completion/request.rs` ask the source's servers, with the focused buffer's savepoint, since the item goes there. `request_signature_help` in `handlers/signature_help.rs` asks the source, and on a refusal closes a signature still shown. The popup keeps the focused buffer's language.
  - `BufferKind::source_open`, the open source under the cursor, without loading it. `Editor::source_open` returns the view's own buffer, or asks its kind. Trigger characters come from its servers: `trigger_auto_completion` in `handlers/completion.rs` and `signature_help_post_insert_char_hook` in `handlers/signature_help.rs`. They read the focused buffer's text before the cursor, so a kind's buffer has to match its source there.
  - `BufferKind::previews_completion`, read by `Completion::new` in `helix-term/src/ui/completion.rs`, and `BufferKind::accept_completion`, which the menu's accept arm calls on a resolved item before turning it into a transaction.
  - `request_incomplete_completion_list` dispatches `EditorWillUpdate { completing: true }` before it asks again, since it runs inside the key that typed the char, before the frame's.
  - `BufferKind::close`, which `buffer_close_by_ids_impl` in `helix-term/src/commands/typed.rs` calls for a kind's buffer, behind `:bc`, `:bca` and `:bco`. Refused with the buffers that hold it open. The default closes the buffer as the core does.
  - `BufferKind::goto_buffer`, which `goto_buffer` in `helix-term/src/commands.rs` asks first, behind `gn`, `gp`, `:bn` and `:bp`. `false` cycles the buffers as usual.
  - `BufferKindUi` in `helix-term/src/kind.rs`, a kind's answers that need the UI. A feature registers one for its kind's name, `BufferKind::name`, with `kind::register` at startup, and `kind::ui` finds a buffer's.
  - `BufferKindUi::reload`, which `reload` in `helix-term/src/commands/typed.rs` calls in place of reading the file, and `reload_all` calls for each open buffer that has one, after the files.
  - `BufferKindUi::write`, which `write_impl` in `typed.rs` calls in place of writing the buffer, behind `:w` and the commands that write through it.
  - `BufferKindUi::keymap`, the keymap while a kind's buffer has focus, built from the user's. `KindKeys` in `helix-term/src/kind.rs` swaps it into `EditorView::keymaps` at the top of `handle_event`, after `EditorWillUpdate`, and the user's back when focus leaves. A whole `Keymaps` swaps, so pending keys and sticky modes stay with the keymap they started in.
  - `BufferKind::gutter`, which `line_numbers`, `diff` and `diagnostic` in `helix-view/src/gutter.rs` ask first, through `kind_gutter`. `None` draws the core's column.
  - `Editor::features` in `helix-view/src/features.rs`, a feature's state by its type, with `get` and `get_or_default`. A field, not a method, so a borrow of a feature's state and one of `editor.documents` can sit side by side.
  - `FeatureTable` in `helix-term/src/config.rs`: a feature's top-level table, and its `apply`, which reads it into the feature's state. `helix-term`'s `Config::features` keeps the registered tables as TOML, merged from both files as `[editor]` is. `Config::load` refuses a top-level name that's neither the core's nor a registered table's, and runs each `apply` against a scratch state, so a bad table refuses the file. `Config::apply_features` reads them into `Editor::features` in `Application::new` and in `refresh_config`, behind `:config-reload`.
  - `Feature` in `helix-term/src/feature.rs`: a feature's static and typable commands. `feature::static_commands` and `typable_commands` list the core's, then each feature's, and every lookup by name goes through them: `MappableCommand::from_str` for the keymap, `TYPABLE_COMMAND_MAP` and the command line's completion in `typed.rs`, the command palette, and `xtask/src/docgen.rs`. `FEATURES` there lists the features Helix is built with. `CommandCompleter`'s constructors are public so a feature can build its typable rows.
  - Tab names: `Tab` in `helix-view/src/tree.rs` is a name a feature declares, as the review's `REVIEW_TAB`. `TABS` in `helix-term/src/commands/typed.rs` lists the ones features register, for `:tab`'s messages.
  - `TabOptions::keeps` in `helix-view/src/editor.rs`, a property `Editor::open_tab` gives a tab: the name of a buffer kind whose buffers the tab keeps. `Editor::send_out_of_kept_tabs` runs on `EditorWillUpdate`, from a hook `handlers::setup` registers before any feature's.
  - `Editor::close_tab`, which closes every view of a tab, and with them the tab.
  - `BufferKindUi::decorate`, which `render_view` in `helix-term/src/ui/editor.rs` calls after the syntax highlights' overlay and before the rainbow brackets'. `ui::text_decorations` is crate-visible so a kind can name `DecorationManager`.
- Book: `book/src/configuration.md` (`[review]`), the first feature table.
- Tests: `config_refuses_a_top_level_table_no_one_reads` in `helix-term/src/config.rs`, and the review's `keys_bind_the_reviews_commands_by_name` in `commands/review.rs`, which reads its commands' names through the registry.
- Core rules for a kind's buffer:
  - A refused close or quit doesn't swap it out of its view to show the first unsaved buffer: `buffer_close_by_ids_impl` and `buffers_remaining_impl` in `typed.rs`.
  - `Editor::switch` in `helix-view/src/editor.rs` never discards it as an empty scratch buffer.
- Relies on:
  - A language feature that asks a server reads its buffer through `Editor::source`. A cherry-pick that adds one, or that adds a read of the focused buffer to a request in one, asks from the source instead.
