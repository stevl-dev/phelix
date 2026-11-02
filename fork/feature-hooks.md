# Helix fork: feature hooks

Snapshot: `main` at `4a60f8c`, on upstream `master` at `ba40e547`.
Goes in `fork/feature-hooks.md`. Feature slug: `feature-hooks`.

Status: phases 0 to 7 done. Section 9's questions are answered under Decisions. Phase 8 is next.

## Bottom line

- **The rule:** the core names no feature. When a feature needs the core to act differently, the core grows a general hook. It never grows an `if` for that feature.
- **Why now:** the review reaches into the core in 113 lines across 17 files. Tabs added 9 more checks. Each was reasonable alone, and the next feature would add its own set. The human chose to do this before the next feature.
- **What changes for the user:** nothing, apart from `[editor.review]` becoming `[review]` (phase 8), as Decisions says. Every phase is a refactor that the review's 57 integration tests already pin.
- **The hooks**, each from a check the review already needs:
  1. Events before each key and each frame.
  2. Buffer kinds. A buffer can carry a kind that answers for it: where a language feature asks from, close, reload, write, buffer motions, gutter, decorations, and keys.
  3. Feature state on the editor, kept by type, not as a named field.
  4. Tabs named by features, with "files opened here go to code" as a property a tab can have.
  5. A command registry that features add commands and picker actions to.
- **The end state:** the review is a crate of its own, `helix-review`. A new `hx` crate holds the binary and the list of features. The compiler then keeps the rule: the core can't name the review, because it can't see it.
- **Not a plugin system.** The hooks are Rust traits and functions in one process. They change together with the core, and the compiler finds every user. A scripting layer could sit on them later. That isn't in this plan.
- **Biggest risk:** phase 2. Language features ask from the buffer a kind names, where today they ask from a view briefly pointed at another buffer ("lending").
- **Size:** about 14 commits. Most phases move code and delete checks. Phases 2, 3, 10 and 11 change how the core works.

## 1. Where the review reaches into the core

Counted at `4a60f8c` with the command in section 7. 113 lines in 17 files, with comments and the tree's tests. Grouped by what each asks the core for:

| What the review needs | Where it reaches in today | Hook | Phase |
| --- | --- | --- | --- |
| Running before each key and frame | `review::sync` at the top of `EditorView::handle_event` and `EditorView::render` (`ui/editor.rs:1458`, `1624`) | An event | 1 |
| Language features from the row's file | `lent()` swaps in `commands/review/lsp.rs`, the view lend in `review/lend.rs` | `BufferKind::source` | 2 |
| Completion and signature help from the row's file | `request.rs:253`, `345`; `completion.rs:136`; `signature_help.rs:109`, `303`; `ui/completion.rs:135`, `234`; the copies in `request/review.rs` and `signature_help/review.rs` | `source`, `source_open`, `previews_completion`, `accept_completion` | 3 |
| Close, refused closes, buffer motions | `typed.rs:195` (`:bc`), `typed.rs:214` and `858` (the switch on refusal), `commands.rs:927` (`goto_buffer`), `editor.rs:1991` (empty scratch discard) | `BufferKind::close`, `goto_buffer`, and two core rules | 4 |
| Reload and write | `typed.rs:1620` (`:reload`), `typed.rs:1708` (`:reload-all`), `typed.rs:405` (the `:w` seam into `typed/review_write.rs`) | `BufferKindUi::reload`, `write` | 5 |
| Its own keys | `EditorView::review_keys` and `ReviewKeys::swap` (`ui/editor.rs:47`, `1459`) | `BufferKindUi::keymap` | 6 |
| Its gutter and drawing | `gutter.rs:57`, `102`, `157`; `ui/editor.rs:131` | `BufferKind::gutter`, `BufferKindUi::decorate` | 7 |
| Its state and config | `Editor::review`, `Config::review` and `ReviewConfig` (`editor.rs:1183`, `1375`) | Feature state, feature config | 8 |
| Its tab | `Tab::Review` and `Tab::ALL` (`tree.rs`), the `:tab` message, `move_out`, `note_views` and `shut` in the review | Tab names; a tab property | 9, 10 |
| Its commands | 7 rows in `static_commands!` and their wrappers (`commands.rs:417`, `3510`–`3535`), 5 rows in `TYPABLE_COMMAND_LIST` | Command registry | 11 |
| Its picker action | `PickerCommand::ReviewJump` (`picker_keys.rs`), `Picker::with_on_review_jump` (`picker.rs`), `changed_file.rs:206` | Picker actions | 12 |
| Its modules | `mod review` lines in `helix-view/src/lib.rs`, `keymap.rs`, `ui/text_decorations.rs`, `commands.rs`, `typed.rs`, `request.rs`, `signature_help.rs` | The crate move | 14 |

What stays in the core, because it's already general:

- `Document::listed` and `Document::name_without_path` (`tabs` phases 5 and 6). They're flags any feature sets, like Vim's `buflisted`. Their doc comments name the review only as an example.
- The tab mechanism in `tree.rs`: root containers in one node map, `open_tab`, `switch_tab`, `quit_view`. The code tab stays a core concept, as the home tab.
- `helix-vcs/src/index.rs` and `git/index.rs`: writing the index is a git operation, not a review one.
- The small features: `picker-keys`, `changed-files-views`, `yank-diagnostics`, `align-message`. They change core behavior and own no buffers, tabs or state, so they're core code in a hard fork. They get no crate and no hooks.

## 2. Rules for this plan

- **Hooks come from evidence.** Every hook here replaces checks the review has today. No hook is added for a feature that doesn't exist yet.
- **The default is today's behavior.** A buffer with no kind takes the exact path it takes now. The hooks' default methods do what the core did.
- **Each phase deletes review checks from the core** and adds no new ones. Section 7 has the count to expect after each.
- **Each phase is a refactor that the existing tests pin.** Where a phase changes behavior on purpose, Decisions names it, and the phase adds the test.
- **Code moves, it isn't wrapped.** When a hook replaces a check, the check and the review's entry point both go. No shims.
- **The review keeps its internals.** This plan moves the review's seams, not its logic. `layout`, `reconcile`, `forward`, `sync`'s engine, `log`, `syntax`, `words` and `stage` stay as they are, apart from how they reach their state.

## 3. Design

The type and method names below are proposals. The phase that adds a hook settles its exact signature, against the call sites it replaces, and records the result in Found.

### 3.1 Events before each key and frame (phase 1)

- `helix-term/src/events.rs` gets one event:

  ```rust
  events! {
      EditorWillUpdate<'a> { editor: &'a mut Editor, completing: bool }
  }
  ```

  `completing` is whether a completion menu is open, as `review::sync` takes it now.
- `EditorView::handle_event` and `EditorView::render` dispatch it where they call `review::sync` today.
- The review registers a hook that calls `review::sync`, from where `helix-term` registers its handlers (`handlers::setup`). It moves to the review's registration in phase 11.
- INVARIANT: the event runs before the keymap swap in `handle_event`. The swap reads which buffer the focused view shows, and the sync can change that.
- Hooks run in registration order. Phase 10's core check has to run before the review's sync, as `move_out` runs first in `sync` today. Phase 10 registers it from the core's own setup, which runs before any feature's.

### 3.2 Buffer kinds (phases 2–7)

A buffer kind is a value a feature defines and a document points at.

```rust
// helix-view/src/kind.rs
pub trait BufferKind: 'static {
    /// Names the kind, for the term side's registry and for `Debug`.
    fn name(&self) -> &'static str;
    // the hooks of 3.3–3.7, each with a default that does what the core does now
}

// helix-view/src/document.rs
pub kind: Option<&'static dyn BufferKind>,
```

- A feature declares its kind as a `static` and sets `doc.kind` when it makes the buffer, as the review sets `listed` and `name_without_path` today.
- A `&'static dyn BufferKind` is `Copy`. A hook taking `&mut Editor` copies the pointer out of the document first, so there's no borrow of the editor to fight.
  INVARIANT: copy `doc.kind` into a local before calling a hook that takes the editor.
- `helix-view` can't name `helix-term`'s types, so the hooks that need a `compositor::Context`, a keymap or decorations are on a second trait, `BufferKindUi` in `helix-term/src/kind.rs`. It sits in a registry keyed by `BufferKind::name`, filled at startup (phase 5).
- A buffer with a kind is never discarded as an empty scratch buffer by `Editor::switch` (`editor.rs:1991`). That replaces the `stream_of` check there with `doc.kind.is_none()`. Phase 4.

### 3.3 Where language features ask from (phases 2, 3)

Today the review "lends" its view: for one command it points the focused view at the row's file, runs upstream's command, and points it back (`commands/review/lsp.rs`, `signature_help/review.rs`). It works because upstream's commands read the focused buffer only before their request goes out. `FORK.md` lists five such assumptions under the review's "Relies on".

This plan has the commands ask the buffer kind instead.

```rust
// on BufferKind
/// The buffer a language feature asks from for `view`'s primary selection,
/// loaded if need be, with `view`'s selection at the same place in it.
/// Or why there's none.
fn source(&self, editor: &mut Editor, view: ViewId) -> Result<DocumentId, String>;

/// The open buffer under `view`'s cursor, without loading it. For checks that
/// run on every typed character, like trigger characters.
fn source_open<'e>(&self, editor: &'e Editor, view: ViewId) -> Option<&'e Document>;

/// Whether the completion menu previews a selected item in the text.
fn previews_completion(&self) -> bool { true }

/// Moves an item being accepted onto the buffer, and takes edits it makes
/// elsewhere.
fn accept_completion(&self, editor: &mut Editor, view: ViewId,
                     item: &mut lsp::CompletionItem, encoding: OffsetEncoding) {}
```

- `Editor::source(view) -> Result<DocumentId, String>` returns the view's own buffer when it has no kind, and asks the kind otherwise. `Editor::source_open(view)` is the same without loading.
- The review's `source` is today's `review::lend` in `review/lend.rs`, with the same refusals. Its `source_open` is today's `review::file_at_cursor`.
- The selection the source puts in the file is keyed by the asking view's id, as `lend` already does with `ensure_view_init`. The view itself keeps showing its own buffer the whole time. That's the change: nothing points the view at another buffer any more.
- What reads the source, and what keeps reading the focused buffer:

  | Code (`helix-term/src/commands/lsp.rs` unless named) | Reads the source | Keeps the focused buffer |
  | --- | --- | --- |
  | `goto_single_impl` (`goto_definition`, `goto_declaration`, `goto_type_definition`, `goto_implementation`) | the request: servers, position, offset encoding | `jump_to_location`'s `push_jump`, so the jump records where you were in the review |
  | `goto_reference` | the request | the same |
  | `hover` | the request | the popup's place, at the review's cursor |
  | `code_action` | the request and the range | the menu's place; the actions edit by path |
  | `request_completions` (`handlers/completion/request.rs`) | servers, the text before the cursor, the trigger context | the savepoint, which belongs to the buffer that's edited |
  | `request_incomplete_completion_list` | the same | the same |
  | `trigger_auto_completion` (`handlers/completion.rs`) | trigger characters, through `source_open` | |
  | `request_signature_help` (`handlers/signature_help.rs`) | the request | the popup |
  | `signature_help_post_insert_char_hook` | trigger characters, through `source_open` | |

- `rename_symbol` keeps the review's own version in `commands/review/lsp.rs`. The review records the rename as a step of its undo log, and upstream's rename doesn't. Phase 2 checks whether upstream's could take a hook for that. If it can't, the review's `lent()` is left handling only `rename_symbol`, and Found says why.
- Not routed, as today: `symbol_picker`, the diagnostics pickers, `workspace_symbol_picker`, `select_references_to_symbol_under_cursor`. They aren't lent today either. The last one would need a map back from the file to the review's rows, which no hook has yet.
- What goes: `commands/review/lsp.rs` apart from the rename, `handlers/completion/request/review.rs` (with its copy of the completion context), `handlers/signature_help/review.rs`, `review::previews_completion`, and the lend's `INVARIANT`s. `review/lend.rs` becomes the review's `source`.
- Signature help's popup keeps reading the focused buffer's language, so in the review it still shows without colors (Decisions, 5).
- `request_incomplete` calls `review::sync(editor, true)` today, because it runs inside the key that typed the character, before the frame's sync. The general path dispatches `EditorWillUpdate { completing: true }` there instead. That's why phase 1 comes first.

### 3.4 Close, motions, reload, write (phases 4, 5)

```rust
// on BufferKind
/// Closes the buffer. Refused with the buffers that hold it open, by name.
fn close(&self, editor: &mut Editor, doc: DocumentId, force: bool)
    -> Result<(), Vec<(DocumentId, String)>>;   // default: Editor::close_document
/// `gn` and `gp` in the buffer. `false` to cycle buffers as usual.
fn goto_buffer(&self, editor: &mut Editor, direction: Direction, count: usize) -> bool { false }

// on BufferKindUi
fn reload(&self, cx: &mut compositor::Context, doc: DocumentId);
fn write(&self, cx: &mut compositor::Context, doc: DocumentId,
         path: Option<&str>, options: WriteOptions) -> anyhow::Result<()>;
```

- `buffer_close_by_ids_impl` (`typed.rs:184`) calls `close` for a buffer with a kind. The review's is `review::close`.
- The switch to the first unsaved buffer on a refused close or quit (`typed.rs:214`, `858`) skips a buffer with a kind. That's a core rule, not a hook: a kind's buffer is never swapped out of its view to show an unsaved file. It's what the review does now, by the human's call at the tabs manual check.
- `goto_buffer` (`commands.rs:925`) asks the kind first. The review's is `review::step_file`.
- `:reload` on a buffer with a kind calls its `reload`. `:reload-all` calls `reload` for every open buffer that has a kind with a UI part, where it calls `review::reload` today.
- `write_impl` (`typed.rs:405`) calls `write` for a buffer with a kind. The seam into `typed/review_write.rs` goes.
- `typed/review_write.rs` is a copy of `write_all_impl`'s save chain. Phase 5 tries to make `write_all_impl` take the documents to write, so the review calls it with its files and the copy goes. The review records the save's own edits as undo steps. If the chain can't give it that, the copy stays as the review's `write`, and Found says why.

### 3.5 Keys (phase 6)

```rust
// on BufferKindUi
/// The keymap while the buffer has focus, built from the user's.
fn keymap(&self, user: &HashMap<Mode, KeyTrie>) -> HashMap<Mode, KeyTrie>;
```

- `EditorView::review_keys` becomes `EditorView::kind_keys`, which holds the user's keymap and which kind's layer is in, while one is.
  A switch from one kind's buffer straight to another's puts the user's keymap back first.
- `ReviewKeys` goes. The review's `bindings()` and `replace_commands()` stay in its keymap module, as the body of its `keymap`.
- After phase 2, `replace_commands` swaps in only the hunk motions, undo and redo, and the rename.

### 3.6 Gutter and decorations (phase 7)

```rust
// on BufferKind
fn gutter<'d>(&self, editor: &'d Editor, doc: &'d Document, theme: &Theme,
              gutter: GutterType, focused: bool) -> Option<GutterFn<'d>> { None }

// on BufferKindUi
fn decorate(&self, editor: &Editor, doc: &Document, view: &View, theme: &Theme,
            decorations: &mut DecorationManager, overlays: &mut Vec<OverlayHighlights>) {}
```

- `line_numbers`, `diff` and `diagnostic` in `gutter.rs` ask the kind first, as they ask `review::gutter` now.
- `render_view` (`ui/editor.rs:131`) calls `decorate` where it calls `text_decorations::review::render`.
- The review still sets `gutters.line_numbers.min_width` on its views from its sync (`fit_gutters`), since `GutterType::width` gets no `&Editor`. That's its own code and stays.

### 3.7 Feature state and config (phase 8)

- `Editor::features: Features`, a map from `TypeId` to `Box<dyn Any>`, with `get::<T>()`, `get_mut::<T>()` and `insert`.
- `Editor::review` goes. The review reaches its state as `editor.features.get_mut::<Review>()`.
  INVARIANT: go through the `features` field, not a method on `Editor`, so a borrow of the state and of `editor.documents` can still be split as they are now (`accept_item` takes `&mut editor.review` beside a document).
- Config, by Decisions 2: a top-level `[review]` table. `helix-term`'s `Config` keeps the top-level tables that features register as raw TOML, keyed by name, and hands each feature its own table at startup and on `ConfigDidChange`. `ReviewConfig` moves into the review and reads `[review]`. A user's `[editor.review]` is refused as an unknown key, as any other is.
  `#[serde(flatten)]` into `helix_view::editor::Config` won't do, since that struct has `deny_unknown_fields`, and serde can't combine the two.
- `helix-term`'s `ConfigRaw` (`config.rs:79`) also has `deny_unknown_fields`, and it stays strict: a top-level table no feature registered, like a typo `[keyss]`, is still an error. So `ConfigRaw` takes the registered names and refuses the rest, by hand or with a second pass over the TOML. The phase settles which.
- Phase 11's registry doesn't exist yet in phase 8. Until then the registered names are a `const` in `helix-term`'s `config.rs`, `&["review"]`, counted by section 7. Phase 11 moves it to `Feature::config`.

### 3.8 Tabs (phases 9, 10)

Phase 9, names:

- `Tab` stops being an enum. It becomes a name, `Tab(&'static str)`, with `Tab::CODE` in the core.
  The review declares `const REVIEW_TAB: Tab = Tab("review")`.
- `Tab::ALL` goes. Its users are `Tree::tabs` (`tree.rs:358`), which filters it for the open tabs, and `:tab`'s lookup of a name (`typed.rs:2167`).
  `Tree::tabs` then lists the active tab and the hidden ones, code first and the rest in the order they opened. The active tab isn't in `hidden`, so the order needs care: the phase settles how. `:tab`'s completer (`completers::tab`) keeps using `Tree::tabs`.
- Features register their tab names, by Decisions 3, so `:tab` keeps its two messages: one for a name no feature registered, one for a tab that isn't open. The first lists the registered names in place of today's fixed "code and review".
  Until phase 11's registry, the registered tabs are a `const` in `helix-term`, `[Tab::CODE, REVIEW_TAB]`, counted by section 7. Phase 11 moves the review's to `Feature::tabs`.
- The fallbacks to the code tab in `Tree::remove` and `Editor::close_document` stay. The code tab is the home tab, so the core may name it.
- `tree.rs`'s unit tests use a tab name of their own instead of `Tab::Review`.

Phase 10, the tab property:

- `Editor::open_tab` takes a `TabOptions { keeps: Option<&'static str> }`, the name of a buffer kind.
  In such a tab, a view that showed a buffer of that kind at the last check and now shows another buffer gives the new buffer to the code tab's focused view, with a jump there and the view's selection, and gets its old buffer back.
  That's `move_out` from `review/sync.rs`, made general. It reads what views show, not which command ran, so it still catches a goto whose answer comes after the key.
- The core keeps the last check's views per tab and runs the check on `EditorWillUpdate`, registered before any feature's hook.
- `Stream::views`, `note_views` and `move_out` leave the review.
- The review stops filtering views on its tab. `shut` closes the tab it opened through a new `Editor::close_tab(tab)`, which closes every view of the tab, as `shut` does by hand now.

### 3.9 Commands and picker actions (phases 11, 12)

Phase 11, commands:

- `helix-term/src/feature.rs`:

  ```rust
  pub struct Feature {
      pub name: &'static str,
      pub commands: &'static [MappableCommand],     // static commands
      pub typable: &'static [TypableCommand],
      pub config: Option<&'static str>,              // its top-level table, phase 8
      pub tabs: &'static [Tab],                      // the tab names it opens, phase 9
      pub register: fn(),                            // hooks, kinds, UI kinds
  }
  ```

- `helix_term::feature::init(features: &'static [Feature])` runs once, at the top of `main`, before the config is read.
  The keymap's names are looked up during deserialization (`MappableCommand::from_str`, `commands.rs:253`), and serde gives that lookup no context. So the registry is a global, set once (`OnceLock`).
  INVARIANT: `init` runs before anything reads a command by name. A second `init` with the same list does nothing, so every integration test can call it.
- Users of the lists change to the registry: `MappableCommand::from_str`, `TYPABLE_COMMAND_MAP`, the command palette (`commands.rs:3548`), and `xtask/src/docgen.rs`.
- The `const`s of phases 8 and 9 go into the review's `Feature`.
- The review's 7 static rows (`review_open` to `review_redo`), their wrappers in `commands.rs`, and its 5 typable rows (`review`, `stage-hunk`, `unstage-hunk`, `review-restore`, `review-context`) move into the review's `Feature`.
- Trap: `static_commands!` makes each row an associated const, like `MappableCommand::review_next_hunk`, and the `keymap!` macro names commands by that path. The review's `bindings()` and `replace_commands()` in `keymap/review.rs` use them. Once the rows leave the macro, the review declares its own `MappableCommand::Static` consts, and its bindings name those. The macro may need an arm that takes a command value, not a name.
- `review::FEATURE` registers the review's hooks, its `BufferKindUi` and its event hook. Until phase 13, the list of features is a `const` in `helix-term`.
- The generated `static-cmd.md` and `typable-cmd.md` keep their content. Their order may change. `cargo xtask docgen` checks that.

Phase 12, picker actions:

- `PickerCommand::ReviewJump` becomes `PickerCommand::Action(&'static str)`. `[keys.picker]` parses a registered action's name to it. A feature registers an action with its default key: the review registers `review_jump` on `Alt-r`.
- The changed-file picker keeps a registry of actions, `fn(&mut compositor::Context, ChangeView, &FileChange)`, and runs the one a key names. `Picker::with_on_review_jump` and `changed_file.rs`'s call to `review::jump` go.
- `picker_keys.rs` keeps its built-in commands. Only the review's goes.

### 3.10 Crates (phases 13, 14)

```
helix-core, helix-view      core: documents, views, tabs, events, BufferKind
helix-vcs                   git, the index writes included
helix-term                  library: compositor, UI, commands, BufferKindUi, the registries
helix-review                the review: depends on helix-view, helix-vcs and helix-term
hx                          lib: the list of features. bin: main.rs
xtask                       depends on hx for docgen
```

Phase 13, `hx`:

- `hx/src/main.rs` is `helix-term/src/main.rs`. `hx/src/lib.rs` has `pub const FEATURES: &[Feature]`.
- `hx/Cargo.toml` takes `[[bin]] hx`, `default-run`, `[package.metadata.deb]`, and the Windows icon from `helix-term/build.rs`. The grammar build stays in `helix-term/build.rs`, since `helix-term`'s tests load grammars.
- The root `Cargo.toml`: `hx` in `members`, `default-members = ["hx"]`, and `hx` in `[profile.integration]`.
- `cargo install --path hx --locked` replaces `--path helix-term` in `book/src/building-from-source.md`, `FORK.md` (Build), and `agents/VERIFY.md`.
- `default.nix` reads the package name from `helix-term/Cargo.toml`. It reads `hx/Cargo.toml` instead.
- `docs/architecture.md` gets an `hx` row.
- `xtask` depends on `hx` and calls `feature::init(hx::FEATURES)` before docgen.

Phase 14, `helix-review`:

- One crate, by Decisions 6. `helix-view/src/review.rs` and `review/` go to a `view` module of `helix-review`, and `helix-term`'s review modules to a `term` module beside it. The phase settles the module names.
- Items of `helix-view` and `helix-term` the review uses but can't see from another crate become `pub`. The compiler lists them. Today at least `Editor::replace_document_in_view` (`pub(crate)`) and the private `request_completions_from_language_server` before phase 3 removes the need.
- The integration harness, by Decisions 4: it moves to `helix_term::test_support` behind the `integration` feature, and the review's tests to `helix-review/tests/integration.rs`. `cargo integration-test` already runs `--workspace --test integration`, so it picks them up.
- `helix-term`'s `Cargo.toml` no longer reaches the review. `hx` depends on `helix-review`.
- `AGENTS.md` (Fork) states the rule, the core names no feature, and points at `helix-review/` instead of the review's old module paths. `agents/REVIEW.md` blocks a diff that names a feature in a core crate. `agents/HELIX.md` (Where to start) gets a row for features and hooks. `agents/TESTING.md` says where a feature's tests go.

## 4. FORK.md

- Phase 0 adds a `feature-hooks` entry: "Plan: `fork/feature-hooks.md`." Each phase adds the hook it makes to the entry, with where it's defined and where the core calls it.
- Each phase shortens the `review-stream` entry's "It hooks in at" list by the seams it removes, and its "Relies on" list by the assumptions it removes. Phase 2 and 3 remove the lending ones. Phase 5 may remove the save chain one.
- Phase 8 updates the `review-stream` entry's `[editor.review]` to `[review]`, with its Book line.
- Phases 9 and 10 update the `tabs` entry: `Tab` as a name, and the tab property.
- Phase 14 updates the `review-stream` entry's Code and Tests paths.

## 5. Plan

Each phase is one commit that builds and passes `tools/fork/land.sh --check`. Each commit marks its phase done here and records what it found.

| # | Status | Commit | Scope |
| --- | --- | --- | --- |
| 0 | done | Add the feature hooks plan. | This file, and the `feature-hooks` entry in `FORK.md`. |
| 1 | done | Run the review before each key and frame through an event. | 3.1. `EditorWillUpdate`, its two dispatches, the review's hook. No test: the review's tests pin the sync, and wiring needs none. |
| 2 | done | Add buffer kinds, and ask the LSP commands' file from them. | 3.2 and the commands half of 3.3. `helix-view/src/kind.rs`, `Document::kind`, `Editor::source`, `source_open`. The review's kind, with `source` from `review/lend.rs`. The `commands/lsp.rs` rows of 3.3's table. `lent()` keeps only the rename. Tests that must pass unchanged: `lsp_keys_in_the_review_ask_from_the_rows_file`, `lsp_keys_in_the_review_need_a_line_the_file_has`, `a_staged_review_refuses_undo_and_lsp_keys`, and the ignored `rust_analyzer` test at a manual run. |
| 3 | done | Ask completion and signature help from the buffer's source. | The rest of 3.3. Delete `request/review.rs` and `signature_help/review.rs`. `previews_completion` and `accept_completion` on the kind. Tests that must pass unchanged: the four completion tests in `review.rs` and `signature_help_in_the_review_asks_from_the_rows_file`. No new test: Decisions 5 keeps the popup as it is. |
| 4 | done | Let a buffer kind close its buffer and step through it. | 3.4's `close` and `goto_buffer`, the two core rules, and `Editor::switch`'s scratch rule. Tests that must pass unchanged: the close and refusal tests, `buffer_motions_in_the_review_step_through_its_files`, `buffer_commands_pass_over_the_review`. |
| 5 | done | Let a buffer kind reload and write its buffer. | 3.4's `reload` and `write`. `helix-term/src/kind.rs` with `BufferKindUi` and its registry. The `:w` seam goes, and the save chain copy if it can. Tests that must pass unchanged: `w_in_the_review_writes_the_files_it_shows_with_unsaved_changes`, the two reload tests, `a_reload_with_no_changes_closes_the_review_and_names_unsaved_files`. |
| 6 | done | Give a buffer kind a keymap of its own. | 3.5. `kind_keys` replaces `review_keys`. Tests that must pass unchanged: `a_custom_key_for_goto_next_change_moves_by_review_hunk`, and every test that types in the review. |
| 7 | done | Draw a buffer kind's gutter and decorations through its hooks. | 3.6. Tests that must pass unchanged: `review_backgrounds_follow_their_keys`, `files_diagnostics_land_on_their_rows_open_or_not`, and the review's tests that read line numbers and signs. |
| 8 | | Keep the review's state and config out of the editor's fields. | 3.7. `Editor::features`. `Config::review` moves to a top-level `[review]`, with the registered table names. Book: `book/src/editor.md`'s `[editor.review]` section becomes `[review]`. Tests in `config.rs`, parsed from TOML: `[review]`'s default and an override, an unknown key in it, an unregistered top-level table refused, and `[editor.review]` refused. |
| 9 | | Name tabs by the features that open them. | 3.8, phase 9 half. `Tree::tabs` without `Tab::ALL`, and the registered tab names. Tests: `tree.rs`'s, with a test tab name, and one for `Tree::tabs`'s order; `tabs.rs` and the `:tab` tests in `review.rs` pass unchanged, since the messages stay. |
| 10 | | Send what opens in a feature's tab to the code tab from the core. | 3.8, phase 10 half. `TabOptions`, the core check, `Editor::close_tab`. Tests that must pass unchanged: `a_file_opened_in_the_reviews_view_lands_in_the_code_tab`, `a_split_opened_on_a_file_in_the_review_tab_stays_there_until_the_review_closes`, `ret_opens_the_file_in_the_code_tab_and_review_comes_back`, and the goto in the `rust_analyzer` test at a manual run. |
| 11 | | Let features register commands. | 3.9, commands. `helix-term/src/feature.rs`, `init`, the registry's users, the review's `Feature`, with phase 8's table name and phase 9's tab. `cargo xtask docgen`. Test: a unit test in `config.rs` parses `[keys.normal]` TOML that binds `review_open` by name, after `init`. It fails if the review's commands aren't in the registry. The `keymap!` macro in the integration tests names commands as Rust paths, not strings, so they don't cover this. |
| 12 | | Let features add actions to the changed-file picker. | 3.9, picker actions. Tests: `picker_keys_overlay_defaults_and_no_op_unbinds` in `config.rs` with `review_jump` as a registered action, its default, an override, and `no_op`. |
| 13 | | Move the binary into an hx crate. | 3.10, `hx`. Build files and the docs that name `--path helix-term`. No new test. `land.sh --check`, and `cargo install --path hx --locked` at the manual check. |
| 14 | | Move the review into a helix-review crate. | 3.10, `helix-review`. The `pub` items, the harness, the tests, `AGENTS.md`, `agents/*.md`, `FORK.md`. Section 7's count reaches 0. |

### Order

- Phase 1 comes first because phase 3 needs its event. It's small and can't break anything the tests don't see.
- Phases 2 and 3 are the riskiest, so they come as early as phase 1 allows. They change what upstream's LSP code reads, which every other phase leaves alone.
- Phases 4–8 are each one hook, and depend only on phase 2's `BufferKind`. Phase 5 adds `BufferKindUi`, which phases 6 and 7 use.
- Phases 9 and 10 depend on phase 1's event and nothing in 2–8.
- Phase 11 needs every hook the review registers to exist, so it comes after 1–10. Phase 12 needs phase 11's registry.
- Phase 13 needs phase 11's `Feature`. Phase 14 needs everything.
- If a phase's subject can't say what it does in one line once its work is known, split it. Phases 2, 5 and 11 are the likeliest to split.

## 6. Risks

| Risk | Where | Guard |
| --- | --- | --- |
| An LSP command reads the focused buffer after its request goes out, so it reads the review where the lend gave it the file | Phase 2 | 3.3's table, checked against each function at phase 2. The view no longer changes what it shows, so a read of the focused buffer gets the review, which is what you see. Any read of the source that 3.3's table misses shows up as "no server" in `lsp_keys_in_the_review_ask_from_the_rows_file`. |
| Completion's savepoint and its servers belong to two buffers | Phase 3 | The review's tests already run that split. They pass `savepoint` from the stream and ask the file. The general path does what `request/review.rs` does now. |
| Hook order: the core's tab check against the review's sync | Phases 1, 10 | The core registers first, from its own setup. The `INVARIANT` sits at the registration. `a_file_opened_in_the_reviews_view_lands_in_the_code_tab` fails if the order flips. |
| The command registry is read before it's filled | Phase 11 | `init` is the first line of `main` and of the test harness. A lookup before `init` panics in debug builds, so it shows in the tests. |
| `pub` items leak internals out of `helix-view` and `helix-term` | Phase 14 | Each one is listed in Found. The ones that are really feature API get a doc comment. The rest are a sign a hook is missing. |
| Cherry-picks from upstream conflict in `commands/lsp.rs`, the completion handlers, `gutter.rs` and `typed.rs` | All | Expected in a hard fork. The `feature-hooks` entry in `FORK.md` lists every call site, so a pick knows what to keep. |

## 7. Measuring progress

The count of lines in the core that mention the review. Run from the repository root:

```sh
git grep -n -i -E '(^|[^p])review' -- helix-term/src helix-view/src \
  | grep -v -E '^helix-view/src/review|^helix-term/src/commands/review|/review\.rs:|review_write\.rs:' \
  | wc -l
```

The `[^p]` leaves out `preview`. At `4a60f8c` it's 113.
Each phase records the count in Found. Phase 14 can't reach 0 by dropping the excludes: after it, the review's files aren't in those crates, so the plain `git grep -i -E '(^|[^p])review' -- helix-term/src helix-view/src` should find only doc comments that name the review as an example. Phase 14 records what's left and why.

## 8. Not in this plan

- A plugin or scripting system.
- The git client, and any other new feature.
- Tabs you make yourself, a tab line, and default keys for tabs.
- Routing the symbol pickers, the diagnostics pickers and `select_references_to_symbol_under_cursor` through a source.
- Splitting `helix-term` further, as into a separate UI library. `hx` is the only split the crate boundary needs.
- Crates for the small features (section 1).
- Renaming the fork.
- Keeping the buffers the review loads in the background out of the buffer lists until you open one yourself, or until the review closes and leaves it open with unsaved changes. Today they show in the code tab's bufferline, `Space b` and `:bn`. It's a behavior change, so it comes as a fix after phase 14, by the human's call, and keeps this plan a refactor.

## 9. Questions for the human

Each changes the plan. Ask them before phase 1, and record the answers under Decisions. The recommended answer is first.

1. **The name.** `feature-hooks` for the plan and the `FORK.md` slug, or something else?
2. **Feature config.** The review's `[editor.review]` table has to leave `helix_view::editor::Config`, or the core names the review.
   - (a) A top-level `[review]` table in `config.toml`, handed to the review as raw TOML. A user's `[editor.review]` stops working. The table is a few days old.
   - (b) Keep `[editor.review]` as the one named spot in the core config, as an exception to the rule.
   - (c) `[editor.review]` read by `helix-term` before `helix_view`'s `Config` sees `[editor]`, so the key stays where it is. This needs `[editor]` read twice.
3. **`:tab` messages.** Without a list of every tab, `:tab` can't tell an unknown name from a tab that isn't open.
   - (a) One message for both: "'x' isn't an open tab. Open tabs: code, review". Simplest, with no registry of tab names.
   - (b) Features register tab names, and the two messages stay as they are.
4. **Where the review's integration tests go in phase 14.**
   - (a) The harness moves to `helix_term::test_support` behind the `integration` feature, and the review's tests to `helix-review/tests/integration.rs`. `cargo integration-test` already runs `--workspace --test integration`, so it picks them up.
   - (b) They stay in `helix-term/tests`, with `helix-review` as a dev-dependency of `helix-term`. Less churn, but the core's tests then reach the feature.
5. **Signature help's highlights.** Today the popup in the review shows the signature without syntax colors, since it reads the focused buffer's language, and that's the review's. With a source, it can read the file's.
   - (a) Use the file's language. A small fix, with a test that the popup has the file's language.
   - (b) Keep it as it is, so phase 3 changes nothing.
6. **The crate layout in phase 14.** One `helix-review` crate with the `helix-view` part and the `helix-term` part as two modules, or two crates? One crate is recommended. The view part has no user but the term part.

## Decisions

By the human, before phase 1:

1. **The name:** `feature-hooks`.
2. **Feature config:** a top-level `[review]` table, as raw TOML handed to the review. Features register their table names, so an unknown top-level table is still an error. `[editor.review]` stops working.
3. **`:tab` messages:** features register their tab names, and `:tab`'s two messages stay.
4. **The review's integration tests:** in `helix-review/tests/integration.rs`, on a harness in `helix_term::test_support` behind the `integration` feature.
5. **Signature help's highlights:** at first, the popup was to use the file's language, with a test. At phase 3 the test couldn't be written: the integration tests run with no language server, and the popup isn't reachable from `Application::editor`. The human dropped the fix, so the popup keeps the focused buffer's language, and shows without colors in the review as before.
6. **Crate layout:** one `helix-review` crate, with the view part and the term part as two modules.

## Found

Each phase records here what it found, the count from section 7, and any hook signature that differs from section 3.

### Phase 0

- The count from section 7 is 113 at `4a60f8c`. Every line reference in sections 1 and 3, and every test the phases name, was checked against `4a60f8c`.
- The first draft said `Tree::tabs` already lists the open tabs in opening order. It filters `Tab::ALL`, so phase 9 rewrites it (3.8).
- `helix-term`'s `ConfigRaw` has `deny_unknown_fields` too, so a top-level `[review]` needs the features' table names to keep unknown tables an error (3.7).
- Phases 8 and 9 come before phase 11's registry, so each keeps its registered names in a `const` in `helix-term` until phase 11.

### Phase 1

- The count from section 7 is 112, from 113. The two `review::sync` calls in `ui/editor.rs` went, and `handlers::setup`'s call to `commands::review::register_hooks` came. That call stays until phase 11 moves it into the review's `Feature`.
- The review's hook is `register_hooks` in `helix-term/src/commands/review.rs`, so the review owns its registration and `handlers.rs` names only the entry point.
- The event is as 3.1 has it: `EditorWillUpdate<'a> { editor: &'a mut Editor, completing: bool }`.
- Under the `integration` feature the hook registry is per tokio runtime (`helix-event/src/runtime.rs`), so each test's app registers its own hook, and tests in parallel don't run the sync twice.
- A dispatch holds the registry's read lock while hooks run, and the sync dispatches events of its own, like `DocumentDidOpen` when it loads a file. Nested reads are safe because hooks register only at startup. The review's tests pass with it.

### Phase 2

- The count from section 7 stays at 112. The lending lived in the review's own files, which the count leaves out, and upstream's `commands/lsp.rs` never named the review. What went is the review's lent goto, references, hover and code action, and the `INVARIANT` that the view showed the review again before anything drew it.
- `BufferKind` has only `source` for now. `name` comes with phase 5, which has the registry it's for, and `source_open` with phase 3, which has its callers.
- The review's kind is `Kind` in `helix-view/src/review.rs`, set on the stream's buffer where `listed` and `name_without_path` are. Its `source` is `review::lend`, which now takes the asking view rather than reading the focus.
- 3.3's table held against the code. Each of `goto_single_impl`, `goto_reference`, `hover` and `code_action` reads the buffer once, before its request goes out, and the answers read the focused view: `jump_to_location`'s `push_jump`, the hover popup, the code action menu. `source` in `commands/lsp.rs` is the one place they ask.
- `rename_symbol` keeps the review's own version, and `lent()` handles only it. Upstream's reads the focused buffer again when you confirm the prompt, and the review records the rename's edit as a step of its undo log. No hook yet runs code around a workspace edit, and adding one for rename alone would be a hook without a second user.
- Completion and signature help still lend: `request/review.rs` and `signature_help/review.rs` call `review::lend` with the focused view. So `lend`'s name, its `WHY`s about jobs and signature help, and `keep_shown`'s note on lent views in `review/sync.rs` stay until phase 3.
- The tests 3.3 names pass unchanged, and so does the ignored `rust_analyzer` test, run by hand.

### Phase 3

- The count from section 7 is 102, from 112. The review's calls went from `request.rs`, `completion.rs`, `signature_help.rs` and `ui/completion.rs`, with their `mod review` lines. `handlers/completion/request/review.rs` and `handlers/signature_help/review.rs` are deleted, and so is the copy of the completion context.
- The signatures are as 3.3 has them. `source_open` and `accept_completion` take the asking view. `accept_completion` takes the editor, so the menu's accept arm copies `doc.kind` out first and takes the view and the document again after it.
- The review's kind answers `source_open` with `file_at_cursor`, `previews_completion` with `false`, and `accept_completion` with `accept_item`, whose split borrow of `editor.review` and `editor.documents` now sits inside the review. The integration test's `accept` calls the kind, as the menu does, instead of `accept_item`.
- `review::lend` is now `review::source`, in `review/source.rs`, since nothing lends the view any more. `lent()` in `commands/review/lsp.rs` is `replacement()`, since it only swaps in the rename. `keep_shown`'s note on lent views in `review/sync.rs` went.
- `request_incomplete_completion_list` dispatches `EditorWillUpdate` for any buffer, not just a kind's, before it asks again. For an ordinary buffer the review's sync finds nothing to do. It returns early when no list is incomplete, before the dispatch, as the review's copy did.
- Trigger characters read the focused buffer's text with the source's servers, as the review's copies did. That's now part of `source_open`'s contract: a kind's buffer has to match its source before the cursor.
- The tests 3.3 names pass, and so does the ignored `rust_analyzer` test, run by hand. Signature help's colors stay as they were (Decisions 5).

### Phase 4

- The count from section 7 is 92, from 102: the close in `buffer_close_by_ids_impl`, the two refusal switches in `typed.rs`, `goto_buffer` in `commands.rs`, and `Editor::switch`'s scratch rule.
- The signatures are as 3.4 has them. `close`'s default closes the buffer as `buffer_close_by_ids_impl` does for an ordinary one, and `goto_buffer`'s returns `false`.
- `step_file` moved from `helix-term/src/commands/review.rs` to `helix-view/src/review.rs`, since the hook is on the `helix-view` side. It called `helix-term`'s `push_jump`, which `helix-view` can't see. It now records its jump with `View::push_jump`, as `Editor::switch` does, after `append_changes_to_history`. It reads the stream from `editor.review` directly: the kind only answers for the review's buffer, so `stream_rows`'s "This buffer isn't a review" refusal can't happen there.
- The review's `close` ignores the `doc` it gets, since the review has one buffer, and keeps its `WHY` that the buffer is never modified, only its files.
- The tests 3.4 names pass unchanged.

### Phase 5

- The count from section 7 is 86, from 92: the `:w` seam and `mod review_write` in `typed.rs`, the review's branch in `reload`, and its read at the end of `reload_all`.
- `BufferKindUi` is in `helix-term/src/kind.rs`, with `reload` and `write` as 3.4 has them. The registry is a static list of `(name, &'static dyn BufferKindUi)`, so the trait is `Sync`. `register` does nothing for a name it has, since every `Application` in the integration tests runs `handlers::setup` in one process. `kind::ui(doc)` finds a buffer's by `BufferKind::name`, which came with this phase.
- The review's name is `review::KIND_NAME`, shared by its kind and its registration. The registration is in `commands::review::register`, which took over from `register_hooks` and registers the event hook too.
- The save chain copy stays. `write_all_impl` could take the documents to write, but the review records all the files' trims as one step of its undo log before any save, and wraps the format callback so the format lands as a step of its own. The chain has no seam for either, and one would be a parameter only the review passes. The copy moved from `typed/review_write.rs` to `commands/review/write.rs`, with the review's other code. `typed.rs`'s three trim functions are `pub(super)` so it can reach them.
- `reload_all` still reads every buffer first, the review's included, then calls each UI kind's `reload`, in the order the review's read ran before.
- The tests 3.4 names pass unchanged.

### Phase 6

- The count from section 7 is 83, from 86: the `review_keys` field, its default, and its swap in `ui/editor.rs`.
- `keymap` returns an `Option`, with `None` as the default, so a kind with no keymap of its own keeps the user's and nothing swaps. 3.5 had it return the map.
- `KindKeys` is in `helix-term/src/kind.rs`, beside the registry. It keeps the user's `Keymaps` and the name of the kind whose keymap is in. On a switch from one kind's buffer to another's it puts the user's back before it builds the next kind's.
- The review's `keymap` in `keymap/review.rs` is `ReviewKeys::swap`'s building of the map, with `replace_commands` and `bindings` as they were. As before, the map is built when the review takes focus, from the user's keymap at that moment.
- The tests 3.5 names pass unchanged.

### Phase 7

- The count from section 7 is 79, from 83: the three gutter branches in `gutter.rs` and the decoration call in `ui/editor.rs`.
- The signatures are as 3.6 has them. The core's three gutters share `kind_gutter` in `gutter.rs`, and `diagnostic` and `diff` now name their `editor` and `is_focused` parameters, since they pass them on.
- The review's kind matches the gutter type to its own `line_numbers`, `signs` and `diagnostics`, so `review::gutter` is a private module now. The decoration is still `text_decorations::review::render`, which the review's UI kind calls. `ui::text_decorations` became `pub(crate)` for that and for `DecorationManager` in the hook's signature.
- `fit_gutters` stays in the review's sync, as 3.6 says.
- The tests 3.6 names pass unchanged.

