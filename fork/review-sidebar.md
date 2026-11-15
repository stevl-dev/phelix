# Helix fork: a file list beside the review

Snapshot: `main` at `ae7b54931`.
Goes in `fork/review-sidebar.md`. Feature slug: `review-sidebar`, part of `review-stream`'s entry in `FORK.md`. The core's new hooks go in the `tabs` and `feature-hooks` entries.
Reference: Hunk's files pane ([hunk.dev](https://www.hunk.dev/)).

Status: approved. Done through phase 6.

## Bottom line

- **Build:** a narrow view at the left of the review tab that lists the review's files as a tree of directories, with each file's status and its added and removed line counts.
- **It's a buffer**, with a kind of its own, in a view of the review tab. Motions, `/`, counts, the mouse, and the user's keymap work in it as in any view.
- **The two views follow each other.** With focus in the review, the list marks the cursor's file. With focus in the list, the review shows the file under the list's cursor, as the cursor moves.
- **It shows by default when the tab is wide enough.** `[review]`'s `sidebar` and `sidebar-width` set that, and `:review-sidebar` shows and hides it.
- **The core grows one concept, side views:** a view with a fixed width, at the left edge of a tab, that doesn't count as the tab's last view. And a tab can keep buffers of more than one kind.
- **Biggest risk:** the list is a real view, so every command that counts, closes, or splits views sees it. Phase 1 has the rules for that, and phase 3 the review's own.
- **Size:** 8 commits. About 120 lines in the core and 450–550 in `helix-review`, plus tests. This is an estimate from reading the code.

## 1. The view

```
 helix-term/src/                │ helix-term/src/ui/editor.rs  modified ──────
   ui/                          │          ⋯ 40 unchanged lines
     editor.rs         M +12 -3 │  41  41    fn render(&self, area: Rect) {
     picker.rs         M  +4 -1 │  42     -      let width = area.width - 2;
   commands.rs         M  +1 -1 │      42 +      let width = area.width.sat…
 helix-view/src/                │  43  43        let height = area.height;
   review.rs           A   +120 │          ⋯ 120 unchanged lines
 README.md             M  +2 -2 │ helix-view/src/review.rs  new file ─────────
```

- A directory row is the directory's name and a `/`. A file row is the file's name. Each level indents two columns.
- A directory with no changed file of its own and one directory in it shares a row with it: `helix-term/src/`.
- The rows come in the review's order. `visible_changes` sorts by `Path`, component by component, which is a tree's order.
  The list walks the review's files in their order and never sorts, so it can't disagree with the review. With another order a directory would show twice, not out of place.
- The text is only the names, so `/` finds a name and a yank is clean. The status letter and the counts are drawn at the view's right edge, not typed into the buffer.
- A renamed file shows under its new path.
- The view has no gutter. It keeps its statusline, which names it `[files]`.

## 2. Design

### 2.1 Side views (core, phase 1)

Today `Tree::recalculate` gives each child of a vertical container the same width (`helix-view/src/tree.rs`), `Tree::split` and `Tree::insert` put a new view after the focused one and focus it, and `Tree::is_last_in_tab` counts every view.

A side view is a view with `View::side: Option<u16>` set, a width in columns.

- **Width.** In a vertical container, a side view takes its width and the others share the rest equally, as today. A side view is capped at half its container, so the others never vanish. With no side view the widths are today's, which the tree's two width tests pin.
- **A container of side views.** A horizontal container whose children all ask for a width asks for the widest. So `C-w s` in a side view stacks two of them in the same column.
- **Opening one.** `Editor::open_side(tab, doc, width)` puts a view of `doc` at the left edge of `tab`'s root and leaves the focus where it was.
- **The last view.** `Tree::is_last_in_tab(id)` is true when every other view of the tab is a side view. So `:q` and `C-w q` in the tab's one main view still quit Helix, as they do in the review today (`Editor::quit_view`, `wclose`, `quit`).
- `Tree::set_side(view, width)` sets or clears it and recalculates when it changed.

Everything else treats it as a view: `C-w w` and `C-w h` focus it, `C-w H` and `C-w L` move it, `C-w q` closes it.

### 2.2 A tab that keeps several kinds (core, phase 2)

`TabOptions::keeps` names one buffer kind. A view of the tab that showed a buffer of the kind and now shows another gives that one to the code tab.

- `keeps` becomes a list of kinds, and the review tab keeps both of its own.
- The check for "now shows another" becomes "now shows a buffer of no kept kind". Without that, showing the review's buffer in the list's view would send the review to the code tab.

### 2.3 The list's buffer (phase 3)

- A scratch document with a second buffer kind, `review-files`. It's unlisted, read-only in the statusline, and goes by `[files]`.
- It lives as long as the review's buffer: `show` makes it and `shut` closes it. Hidden means no view shows it.
- `Stream` holds its rows, one per line: a directory and the first file under it, or a file's index in `files`.
- It's rebuilt where `files` changes, in `update`: at `:review`, `:reload`, and after a stage. Each view's cursor stays on its file, by path.
- The sync holds its text. An edit is put back in the frame it was made, with the review's own message, and its history is cleared, as for the review's buffer.
- Its kind's answers:

  | Hook | Answer |
  | --- | --- |
  | `source`, `source_open` | none: the list shows no code |
  | `close` (`:bc`) | hides the list. The review stays |
  | `goto_buffer` (`gn`, `gp`) | steps between file rows |
  | `gutter` | the core's. The sync empties the view's gutter layout, as `fit_gutters` sets the review's width |
  | `reload` (`:reload`) | reads the review's changes again |
  | `write` (`:w`) | writes the review's files, as `:w` in the review |
  | `keymap` | the user's, with `Enter` on `review_goto_file` |
  | `decorate` | 2.6 |

### 2.4 Showing and hiding (phases 3 and 8)

- Shown means a view of the review tab shows the list. `:review-sidebar` toggles it, and `:review-sidebar show` and `hide` set it.
- Closing the list's view by any key hides it: `C-w q` in it, `:bc` in it, `C-w o` in the review.
- The sync keeps these each frame:
  - A view of the review tab that shows the list is a side view of the configured width with no gutter. A view that stopped showing it isn't.
  - When no view of the tab shows the review, the list's views close. If one is the tab's only view, it shows the review instead. `C-w o` in the list gets there.
- `review::switch_to` focuses a view that shows the review when the tab has one. Today it puts the review in the focused view, which could be the list's.

### 2.5 Following (phases 4 and 5)

Which way it goes depends on where the focus is, so the two can't chase each other.

- **Focus in the review.** The current file is the one of the primary cursor's row. When it changes, the list's cursor goes to that file's row and the view scrolls to show it.
- **Focus in the list.** The file is the one under the list's primary cursor, or a directory's first. When it isn't the file of the review's cursor, the review's cursor goes to that file's header, aligned to the top.
  A click, `j`, `/name`, and `gn` all move the cursor, so each one jumps.
- Entering the list changes nothing, since its cursor is already on the review's file.
- The first move of a visit to the list records the review's place as a jump in the review's view, so `C-o` there goes back.
- `Enter` in the list (`review_goto_file`) focuses the review's view.
- "The review's view" is the view of the tab that last had focus while showing the review. The sync notes it.

### 2.6 Drawing (phases 4 and 6)

- The current file's row has a tint, focused or not: `ui.review.current`, falling back to `ui.selection`.
- Directory rows take `ui.text.directory`.
- At the right edge of a file row: the status letter, then the counts. `M` modified, `A` added, `?` untracked, `D` deleted, `R` renamed, `U` conflict.
  The letter takes `diff.plus`, `diff.delta`, or `diff.minus`, and a conflict `error`. `+N` takes `diff.plus` and `-N` `diff.minus`.
- The counts are the sizes of the file's hunks, which the review already has. A binary file, or one too large to diff, shows none.
- They're drawn each frame from the review's state, over the tail of a name that's too long. So an edit in the review changes them without a write to the list's buffer.

### 2.7 Config and commands (phases 3 and 8)

| Key | Description | Default |
| --- | --- | --- |
| `sidebar` | `true` shows the file list, `false` hides it, and `"auto"` shows it while the review's tab is at least 120 columns wide | `"auto"` |
| `sidebar-width` | The list's width in columns | `30` |

- `"auto"` follows the terminal: the list closes when the tab gets narrower than 120 columns and comes back when it's wide again.
- `:review-sidebar`, or closing the list's view, overrides the config until the review closes.
- `:config-reload` reads both keys again.
- No default key. Bind it by name, like `[keys.normal.space] e = ":review-sidebar"`.

## Plan

Each phase is one commit that builds and passes its tests, and marks itself done here.

| # | Status | Commit | Work |
| --- | --- | --- | --- |
| 0 | Done | Add the plan for the review's file list. | This file. |
| 1 | Done | Add side views to the tree. | 2.1: `View::side`, the widths in `Tree::recalculate`, `Tree::set_side`, the left-edge insert, `Editor::open_side`, and `is_last_in_tab`. Unit tests in `tree.rs`: a side view's width beside one and two others, the cap at half, a horizontal container of side views, the left edge and the kept focus, and `is_last_in_tab` with a side view on either side of the question. The two existing width tests pass unchanged. `tabs`' entry in `FORK.md`. |
| 2 | Done | Let a tab keep buffers of several kinds. | 2.2: `TabOptions::keeps` as a list, and the "no kept kind" check in `send_out_of_kept_tabs`. The review passes its one kind. A unit or integration test, whichever the phase finds can build two kinds: a view that goes from one kept kind to another stays, and one that goes to a file is sent out. `feature-hooks`' entry in `FORK.md`. |
| 3 | Done | List the review's files in a side view. | 2.3 and 2.4 without the config: `view/files.rs` with the tree's rows, the list's kind, the hold in `sync.rs`, the invariants, `switch_to`, and `:review-sidebar`. Hidden until the command shows it, so nothing changes for a review that doesn't run it. Unit tests in `files.rs`, table-driven: nested directories, a shared row, a file at the root, a rename, one file, and rows in `files`' order. Integration tests: the command shows and hides it, an edit to the list is put back, `:q` in the review with the list open quits, `C-w o` in the list leaves the review, `:e` in the list opens in the code tab, and a `:reload` after a new change adds its row and keeps the cursor's file. Book and `FORK.md`. |
| 4 | Done | Mark the review's current file in the list. | 2.5's first half and the tint of 2.6. Integration tests: moving to another file in the review moves the list's cursor, with the row far enough down to need a scroll, and a cursor on a file's last row still marks that file. `book/src/themes.md` for `ui.review.current`. |
| 5 | Done | Show the file under the list's cursor in the review. | 2.5's second half, and `review_goto_file` on `Enter`. Integration tests: `j` in the list puts the review's cursor on the next file's header, a directory row gives its first file, entering and leaving the list without moving leaves the review's cursor where it was, `C-o` in the review goes back after a visit, and `Enter` focuses the review. A unit test that the key binds by name, beside the review's. Book. |
| 6 | Done | Draw each file's status and line counts in the list. | 2.6's right edge, in `term/files.rs`. Unit tests of the pure part: the counts of a file's hunks, and none for a binary file. A render test only if `term/decorations.rs`' test style reaches it. Book. Colors are for the human to check. |
| 7 | | Read the list's width from `[review]`. | `sidebar-width`, read by the sync so `:config-reload` resizes an open list. Unit test of the table, beside the review's: the default against a literal, an override, and a refused value. `book/src/configuration.md`. |
| 8 | | Show the review's file list when the tab is wide. | `sidebar`, `"auto"`, and the override of 2.7. This is the commit that changes what `:review` shows. Integration tests: a wide tab opens with the list and focus in the review, `sidebar = false` opens without it, a hide survives a resize, and a narrow tab opens without it. The harness's terminal is 120 columns wide, so the review's existing tests get the list. A test that fails for that reason is fixed, or sets `sidebar = false` when the list is beside its point. Book and `FORK.md`. |

Phases 1 and 2 are the core's, and nothing else can start without them.
Phase 3 has the rules for a list that is a real view, which is the risk, so it comes straight after.
Phases 4 and 5 are the follow, one direction each, so a fault in one is found alone.
Phase 8 is last, so the default changes only once the list is whole.

After each phase from 3 on, I'll give the paths to check in `phx`, and offer to run the ones the screen's text shows, as `agents/VERIFY.md` says. Colors and the mouse are the human's.

## Decisions (2026-11-15)

The human's answers:

1. The list is a buffer in a view of the review tab, not a drawn strip.
2. The files are grouped by directory.
3. The review follows the list's cursor as it moves, not only on `Enter`.
4. It shows by default when the tab is wide. A config key and commands control it.

Assumed here. Approving the plan approves them, so say which to change:

5. Grouped means a nested tree with shared rows for chains of directories, not one row per directory with its full path. The tree fits 30 columns, and its order is the review's.
6. Directories don't fold.
7. One typable command, `:review-sidebar [show|hide]`, with no default key. `Enter` in the list is a static command, `review_goto_file`. Focus moves with the window keys, like `C-w h`.
8. `"auto"` means at least 120 columns, and follows the terminal as it resizes. The list is 30 columns wide.
9. A hide by hand lasts until the review closes, and the next `:review` asks the config again.
10. `:q` in the review with the list open quits Helix, as it does today without it.
11. `:bc` in the list hides the list. `:bc` in the review closes both.
12. The status letter and the counts are drawn, not text, so `/` and a yank see only names.
13. `u` in the list does nothing. It doesn't undo the review's edits from there.

## Not in this plan

- A filter that narrows the list, as Hunk's has. `/` searches it.
- Folding directories.
- A mark for a file that's staged or partly staged.
- Marks for files with diagnostics.
- The list on the right by config. `C-w L` moves it there.
- A statusline of its own for the list, like a file count. That needs a new hook.
- A list for the code tab. The side view is general, and nothing else uses it yet.

## Risks

- **Commands that count views.** `C-w o`, `C-w t`, `C-w v`, and the swaps all reach the list's view. 2.1 and 2.4 say what each leaves, and phase 3 tests the ones that could strand the tab. `C-w t` in the tab's root stacks the list above the review at half the height until it's pressed again.
- **Soft wrap.** A document takes soft wrap from the editor's config, with no setting of its own. With `soft-wrap.enable`, a long directory row wraps in the list. The rows stay right, since they go by the buffer's lines. If it reads badly, the fix is a general way for a buffer to turn wrap off, in a commit of its own.
- **The statusline in 30 columns.** It shows the mode and `[files]`, and cuts off the rest. Seen in use, it may want the hook that's left out above.
- **Two views of the review.** The list drives the one that last had focus. A second view of the review doesn't move.
- **The existing tests at phase 8.** They run in a terminal exactly as wide as the threshold. That's a wide check of the list for free, and it may cost changes to tests that count views.

## Found

- Phase 1: the left-edge insert is `Tree::insert_side`, which takes a tab that has a view, since the focus has to stay on one.
  Several side views in one container take half of it between them: each is capped at an equal share of the half. The plan's cap only covered one.
  A container with nothing but side views shares its width as any views do, so a tab left with only the list still fills the screen.
  `Tree::swap_split_in_direction` now recalculates. It swapped the two views' areas, which gave a side view the other's width after `C-w L`.
  `Editor::open_side` has no test of its own. Phase 3's tests are the first to open one through it.
- Phase 2: the test is an integration test in `helix-term/tests/test/tabs.rs`, since a unit test can't build an `Editor`. It declares two kinds of its own and calls `send_out_of_kept_tabs` itself, so it doesn't wait on a frame.
  A tab with an empty list keeps nothing, as `None` did.
- Phase 3: the list's text starts at column 0, with two columns a level. The picture in section 1 has a blank column before it, which I took for the drawing's margin, since the text is only the names. For the human to judge on screen.
  Paths are the ones the headers show, relative to the working directory. A repository somewhere else gets one row for its whole path, with every file under it.
  A hidden list's buffer isn't written, since a buffer's history is committed through a view of it. `List::stale` notes that the text changed, and the sync writes it once a view shows the list.
  A new view of the list starts at the top. Phase 4 puts it on the review's file.
  `:review-sidebar show` refuses when no view of the tab shows the review, since the sync would close the list at once.
  Typing in the list puts the first char back and leaves insert mode, so the chars after it run as normal mode keys. A refused edit in the review does the same.
  With only the list left in a hidden review tab, its views close and the tab with them, since showing the review there would take the focus. Nothing reaches that state today.
  The review's `step_file` became `step_rows`, which the list's `gn` and `gp` share.
  The tests' `repo` makes a file's directories. A test that ends with the focus in the list uses `run_until`, since the harness's closing `:q!` closes only the list's view.
  Two tests the plan didn't list: `:bc` in the list hides it, and `gn` and `gp` step between file rows.
- Phase 4: the follow isn't on a change of the file. On every sync with the focus outside the list, a list cursor that isn't on the current file's row goes there. So a new view of the list starts on that row too, and entering the list finds its cursor there whatever moved it before.
  The current file is the one under the cursor of the review's view, which the sync notes: the tab's focused view if it shows the review, or else the one noted last. Phase 5 uses the same view.
  The line below the review's last row counts as the last file's.
  The tint has a render test, which reads the drawn rows' backgrounds with `ui.review.current` set. The fallback to `ui.selection` has none, since the selection draws in that color too.
  The scroll test's repository has 200 changed files, since the test terminal is 150 rows tall.
- Phase 5: both directions are one function, `follow`, which picks its way by where the focus is.
  It runs before the sync saves the review's selections, so a put back edit doesn't bring an older cursor with it.
  `Enter` only moves the focus. The review already shows the file, and a cursor on the row of the review's own file leaves the review where it was.
  `review_goto_file` outside the list says so, as the review's commands do outside the review.
  The list's keymap is the user's with `Enter` over it. The review's swaps, like `u` for `review_undo`, aren't in it.
  The `C-o` test moves through the list twice before it leaves, so it fails if each move records a jump.
- Phase 6: the marks are a `Decoration` whose `render_virt_lines` draws on the row itself. `decorate_line` runs before the row's text, so a long name would cover them.
  The counts' column is as wide as the widest counts of any file, so the letters line up, and the counts sit flush with the view's right edge. A blank comes before the letter, over a long name.
  The blanks are drawn with a default style, so the current file's tint runs under the mark. The tint's test now asks for the tint in all 30 columns.
  A file with no text changes has counts of zero, and shows only its letter, as a binary file does.
  A renamed file's letter takes `diff.delta`, not `diff.delta.moved`, which the plan didn't name.
  There is a render test: the screen-reading style of phase 4's tint test reaches the list, where `term/decorations.rs`' unit tests don't. It reads four rows off the screen, one with a name that's cut, and then an edit in the review that changes a count while the list's text stays.
  `File::counts` and its unit test are in `view.rs`, where `File` is, and `Stream::changes` hands them to the drawing.
