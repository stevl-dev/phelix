# Helix fork: tabs

Snapshot: `main` at `d6ed5bf05`.

Status: approved 2026-10-26. Phase 1 done 2026-10-26.

## Bottom line

- A tab is a layout of its own, with its own splits and focus. Features open tabs, and you don't make your own yet.
  Two exist: `code`, where Helix starts, and `review`. A later git client gets one the same way.
- `:review` opens the review in its tab. `Enter` there switches to the code tab and opens the file, and so does a goto, a picker, or `:e`. `:review` or `:tab review` switches back, and `:tab code` goes to code without opening a file.
- The review leaves the buffer lists: the bufferline, `Space b`, `:bn`, and `:bca`. The statusline calls it `[review]`, not `[scratch]`.
- **How:** tabs are root containers in the tree's one node map. View ids stay unique, and the code that walks every view keeps working.
  What's drawn, clicked, and closed together follows the active tab.
- **Biggest risk:** closing and quitting, in phase 1.

## 1. Tabs in the tree

- `Tree` keeps one root container per tab in its `SlotMap<ViewId, Node>`, with each tab's last focus.
  `root` and `focus` stay the active tab's. `traverse`, `recalculate`, `prev`, `next`, `find_split_in_direction`, `transpose`, `swap_split_in_direction`, `insert`, and `split` all start from them, so they act on the active tab as they are.
- A tab's kind comes from a fixed enum in `helix-view`: `Code` and `Review`. A feature that needs a tab adds a variant.
- Switching saves the active tab's focus, takes the other's root and focus, and recalculates its areas.
  A hidden tab's areas go stale, and a resize recalculates only the active tab, so a switch always recalculates.
- A switch is a focus change, so it does `Editor::focus`'s bookkeeping: normal mode, the old buffer's changes into its history, jumps synced, `DocumentFocusLost`.
- Not one `Tree` per tab. Each would hand out view ids from its own map, and documents keep selections by view id, so ids from two trees could collide.
  The 28 places that walk `views()` would also miss a hidden tree.
- The `// fullscreen: bool` stub goes.

## 2. What follows the active tab

`views()` and `views_mut()` keep walking every view in every tab.
Most callers are bookkeeping and want that: closing a buffer, jumplists, diagnostics, inlay hints, a config reload, and the review's sync.
`:bco --skip-visible` keeps the buffers that `views()` shows, so a buffer shown in a hidden tab counts as visible.

These change to the active tab, through `traverse()`:

- The draw loop in `EditorView::render` (`ui/editor.rs:1650`). It would draw a hidden tab's views over the active one.
- The mouse's hit tests (`ui/editor.rs:1224` and `1236`). They would find a hidden view under the pointer.
- `wonly` (`commands.rs:5970`). It would close the other tabs' views.

## 3. Closing and quitting

- `:q` on a tab's last view quits Helix, in any tab. So do `:q!`, `:wq`, `:x`, and `Ctrl-w q` (`wclose`). On a split they close the split, as now.
- Quitting from a tab's last view runs the unsaved check over every buffer, as now from the tree's last view, then closes every view in every tab, as `:quit-all` does.
  The check is now `views().count() == 1` (`typed.rs:123`, `commands.rs:5960`), which would count the hidden tabs' views. It becomes: the view is its tab's last.
- Helix quits when no tab has a view. `should_close` checks every tab, where it now checks the one tree.
- A buffer close that takes the last view of a tab other than `code` drops the tab and switches to `code`. That's how the review closes: `:bc` in it, as now.
- `close_document` gives the tree a view again when its last view went (`editor.rs:2221`).
  With tabs it checks the code tab, hidden or not, and gives that one the view. `:bca` from the review tab can empty a hidden code tab.
- `Editor::focus` on a view in a hidden tab switches to that tab first. Today only the mouse calls it, with a view it found on screen, so this is a guard.
- `:quit-all` closes every view in every tab, as now.

## 4. The review's tab (phase 2)

- `:review` opens the review tab with one view of the stream and switches to it. With a review open, it switches to the review's tab.
- `Enter` (`review_open`) switches to the code tab and opens the row's file in its focused view.
  It records a jump there first, so `Ctrl-o` in the code tab goes back to where you were in code. On a row of unchanged lines, it still shows them in the review.
- The changed-file picker's `review_jump` switches to the review tab.
- `:bc` in the review closes it, and the buffers it loaded go with it as they do now. Its views close with the buffer, so the tab goes too.
- `:q` on the review tab's last view quits Helix (section 3).
- About ten tests in `helix-term/tests/test/review.rs` come back with `Ctrl-o` after `Enter`. They come back with `:review` instead.

## 5. What opens from the review (phase 3)

A goto, a picker, `:e`, `gf`, `ga`, or `:new` in the review's view lands in the code tab, as `Enter` does.

- A check in `review::sync`, which runs before every frame, finds a view of the review tab that showed the stream and now shows another buffer.
  It gives that buffer, at the view's selection in it, to the code tab's focused view with a jump there, puts the stream back in the review's view, and switches to the code tab.
- The check reads what views show, not which command ran, as the read-only check does. So a goto whose answer comes after the key moves too, and so do pickers and typable commands, with no hook in each.
- A split you open with a file in the review tab, as `:vs` or a picker's `Ctrl-v` does, stays there. The review's own views always show the review, and you can still put a file beside it.
- The lends point the review's view at a file only for one command or request, and the check never runs inside one. The lend's `INVARIANT` already holds that.
- `Editor::switch` keeps its condition that spares the stream when a view switches away from it. The check needs the stream still open to put it back.

## 6. Switching (phase 4)

- `:tab <name>` switches to an open tab: `code`, or `review` while a review is open. It completes the open tabs' names.
  Without a name, it says which tab is active.
- No default keys. `:tab code` and `:tab review` can be bound in `[keys]` like any typable command.

## 7. Buffer lists (phase 5)

- `Document` gets a `listed` flag, like Vim's `buflisted`. The stream's is off, and a git client's buffers would turn theirs off too.
- Unlisted buffers are left out of the bufferline, the buffer picker, `:buffer`'s completion, `goto_next_buffer` and `goto_previous_buffer` (`gn`, `gp`, `:bn`, `:bp`), and `:buffer-close-others` and `:buffer-close-all` with their `!` forms.
- Trap: `goto_buffer` cycles the buffers with `skip_while(id != current)`. From an unlisted buffer, a filtered list never holds the current one, and the loop never ends.
  It has to find its place in the full list, then skip the unlisted ones.
- `:wa` and the quit checks need nothing, since the stream is never modified.

## 8. The name (phase 6)

- `Document` gets a `name` for a buffer with no path: `[review]` for the stream.
  `display_name`, the statusline's three file name elements, and `%{buffer_name}` use it before `[scratch]`.
- The bufferline, the buffer picker, and `:buffer`'s completion print `[scratch]` too. They never show the stream once it's unlisted, so they need nothing.

## Plan

Each phase is one commit. Each commit marks its phase done here.

| # | Status | Commit | Scope |
| --- | --- | --- | --- |
| 0 | Done | Add the tabs plan. | This file. |
| 1 | Done | Let the tree hold several tabs. | Sections 1–3. `Code` and `Review` kinds, switching, dropping an emptied tab, the quit checks, `close_document`'s new view, `Editor::focus` across tabs, and the draw loop, mouse, and `wonly` on the active tab. No command opens a second tab yet, so nothing changes for the user. Unit tests in `tree.rs`: a hidden tab's views keep their ids and stay in `views()` but not `traverse()`, removing a tab's last view drops it and restores the code tab's focus, and `next` and `find_split_in_direction` stay in the active tab. Integration tests open a second tab through the editor's API: `:q` on either tab's last view checks for unsaved buffers and quits, `:q` on a split in it closes only the split, `:bc` of its only buffer drops it and leaves Helix running, and `:bc` of the code tab's only file from it leaves the code tab a view. A `tabs` entry in `FORK.md`. |
| 2 | | Open the review in a tab of its own. | Section 4. `review::show`, and `open` and `jump` in `commands/review.rs`. Integration tests: `:review` leaves the code tab's view on its file, `Enter` switches to code and opens the row's line, `:review` comes back with the cursor where it was, and `:bc` in the review closes it, its tab, and the buffers it loaded. Book: `keymap.md` (Review). `FORK.md`: the `review-stream` entry. |
| 3 | | Move what opens from the review to the code tab. | Section 5. The check in `review/sync.rs`. Integration tests: `:e` in the review's view opens the file in the code tab and leaves the review in its view, a goto against rust-analyzer does the same once its answer comes (in the ignored `rust_analyzer.rs`), and `:vs` with a file stays in the review tab. Book: `keymap.md` (Review). |
| 4 | | Add :tab to switch tabs. | Section 6. The command row and `cargo xtask docgen`. Integration tests: `:tab code` and `:tab review` switch, and `:tab review` with no review fails with its message. Book: `keymap.md` (Review), generated `typable-cmd.md`. |
| 5 | | Leave the review out of the buffer lists. | Section 7. Integration tests: `:bn` skips the review from code and leaves it from the review, and `:bca` leaves it open. Book: `keymap.md` (Review). |
| 6 | | Name the review buffer. | Section 8. Integration test: the statusline's drawn row says `[review]`, read as `align-message`'s test reads it. |

Phase 1 is the riskiest, and everything else stands on it. Phase 3 is next, since its check runs on every frame. Phases 5 and 6 don't depend on each other.

## Decisions (2026-10-26)

1. **The name** is `tab`, after Vim's tabpages.
   `perspective`, `desk`, and `screen` were weighed. "Space" reads as Space mode, and the book's other uses of "tab" are the key and the character (`tab-width`, `[editor.smart-tab]`).
2. **`:q` on a tab's last view quits Helix**, in any tab, not like Vim, which closes the tab and goes to another.
   The review closes with `:bc`, as now, and not with `:q`.
3. **What opens in the review's view moves to the code tab**, like `Enter` (section 5). A split you open with a file stays in the review tab.
   Leaving it in the review tab was weighed, and so was moving only the gotos. Both make the gotos act unlike `Enter`, and the second needs the same check anyway.
4. **No tab element in the statusline.** The `[review]` name says where you are (section 8).
   A `tab` element was weighed. It waits for a tab whose buffers don't name it, which the git client may never have.
5. **`:bca` and `:bco` leave the review open** (section 7). Commands that list or sweep buffers skip it, and `:bc` in it still closes it.
   Closing it like any buffer was weighed. `:bco` from the code tab would then close a review open in another tab.

## Not in this plan

- Tabs you make yourself, a tab line, default keys, and a statusline element.
- The git client.

## Found

- Phase 1: two more places assumed one tree, beyond section 2's list.
  `Editor::switch` deleted an empty scratch buffer unless another view of the active tab showed it. It now checks every tab's views, or a scratch shown only in a hidden tab would go from under it.
  `Tree::remove` merged a lone child into its grandparent unless the parent was the active root. For a hidden tab's root, that removed the root itself. It now checks against the view's own tab's root.
  `Editor::focus` switches tabs before `ensure_cursor_in_view`, which moved below the jump sync, so the cursor scrolls by the view's current area.
  The plan's `:bca` test became `:bc <path>`. Until phase 5, `:bca` closes the review's buffer too, which closes the review tab, so the hidden code tab is never left empty.
  `buffers_remaining_impl` switches the focused view to the first modified buffer when the focused one isn't. In the review tab that's the review's view, which phase 3's check will move to the code tab.
  The tests fail with each of these put back: the root check in `remove`, a hidden tab's focus moving off a removed view, `quit_view`'s close of every view, and `close_document`'s check of the code tab.
