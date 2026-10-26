# Helix fork: tabs

Snapshot: `main` at `d6ed5bf05`.

Status: proposed 2026-10-26. It waits for the human's approval and the answers to the questions below.

## Bottom line

- A tab is a layout of its own, with its own splits and focus. Features open tabs, and you don't make your own yet.
  Two exist: `code`, where Helix starts, and `review`. A later git client gets one the same way.
- `:review` opens the review in its tab. `Enter` there switches to the code tab and opens the file. `:review` or `:tab review` switches back, and `:tab code` goes to code without opening a file.
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

These change to the active tab, through `traverse()`:

- The draw loop in `EditorView::render` (`ui/editor.rs:1650`). It would draw a hidden tab's views over the active one.
- The mouse's hit tests (`ui/editor.rs:1224` and `1236`). They would find a hidden view under the pointer.
- `wonly` (`commands.rs:5970`). It would close the other tabs' views.

## 3. Closing and quitting

- Closing the last view of a tab other than `code` drops the tab and switches to `code`.
- Helix quits when the code tab has no views. `should_close` checks the code tab, where it now checks the whole tree.
- `quit` and `wclose` check for unsaved buffers when the close would quit Helix.
  Today that check is `views().count() == 1` (`typed.rs:123`, `commands.rs:5960`), which would count the hidden tabs' views. It becomes: the view is the code tab's last.
- `close_document` gives the tree a view again when its last view went (`editor.rs:2221`).
  With tabs it checks the code tab, hidden or not, and gives that one the view. `:bca` from the review tab can empty a hidden code tab.
- `Editor::focus` on a view in a hidden tab switches to that tab first. Today only the mouse calls it, with a view it found on screen, so this is a guard.
- `:quit-all` closes every view in every tab, as now.

## 4. The review's tab (phase 2)

- `:review` opens the review tab with one view of the stream and switches to it. With a review open, it switches to the review's tab.
- `Enter` (`review_open`) switches to the code tab and opens the row's file in its focused view.
  It records a jump there first, so `Ctrl-o` in the code tab goes back to where you were in code. On a row of unchanged lines, it still shows them in the review.
- The changed-file picker's `review_jump` switches to the review tab.
- Closing the review tab's last view closes the review, and the buffers it loaded go with it as they do now.
  Closing the stream's buffer (`:bc`) closes its views, so the tab goes too.
- A goto, a picker, or `:e` in the review tab opens there, as in any view, and `Ctrl-o` comes back (question 4).
- `Editor::switch` keeps its condition that spares the stream when a view switches away from it. `:e` in the review tab needs it.
- About ten tests in `helix-term/tests/test/review.rs` come back with `Ctrl-o` after `Enter`. They come back with `:review` instead.

## 5. Switching (phase 3)

- `:tab <name>` switches to an open tab: `code`, or `review` while a review is open. It completes the open tabs' names.
  Without a name, it says which tab is active.
- No default keys. `:tab code` and `:tab review` can be bound in `[keys]` like any typable command.

## 6. Buffer lists (phase 4)

- `Document` gets a `listed` flag, like Vim's `buflisted`. The stream's is off, and a git client's buffers would turn theirs off too.
- Unlisted buffers are left out of the bufferline, the buffer picker, `:buffer`'s completion, `goto_next_buffer` and `goto_previous_buffer` (`gn`, `gp`, `:bn`, `:bp`), and `:buffer-close-others` and `:buffer-close-all` with their `!` forms.
- Trap: `goto_buffer` cycles the buffers with `skip_while(id != current)`. From an unlisted buffer, a filtered list never holds the current one, and the loop never ends.
  It has to find its place in the full list, then skip the unlisted ones.
- `:wa` and the quit checks need nothing, since the stream is never modified.

## 7. The name (phase 5)

- `Document` gets a `name` for a buffer with no path: `[review]` for the stream.
  `display_name`, the statusline's three file name elements, and `%{buffer_name}` use it before `[scratch]`.
- The bufferline, the buffer picker, and `:buffer`'s completion print `[scratch]` too. They never show the stream once it's unlisted, so they need nothing.

## Plan

Each phase is one commit. Each commit marks its phase done here.

| # | Status | Commit | Scope |
| --- | --- | --- | --- |
| 0 | Done | Add the tabs plan. | This file. |
| 1 | | Let the tree hold several tabs. | Sections 1–3. `Code` and `Review` kinds, switching, dropping an emptied tab, the quit checks, `close_document`'s new view, `Editor::focus` across tabs, and the draw loop, mouse, and `wonly` on the active tab. No command opens a second tab yet, so nothing changes for the user. Unit tests in `tree.rs`: a hidden tab's views keep their ids and stay in `views()` but not `traverse()`, removing a tab's last view drops it and restores the code tab's focus, and `next` and `find_split_in_direction` stay in the active tab. Integration tests open a second tab through the editor's API: `:q` on its last view doesn't quit, `:q` on the code tab's last view checks for unsaved buffers, and `:bca` from it leaves the code tab a view. A `tabs` entry in `FORK.md`. |
| 2 | | Open the review in a tab of its own. | Section 4. `review::show`, and `open` and `jump` in `commands/review.rs`. Integration tests: `:review` leaves the code tab's view on its file, `Enter` switches to code and opens the row's line, `:review` comes back with the cursor where it was, and closing the review tab closes the review and the buffers it loaded. Book: `keymap.md` (Review). `FORK.md`: the `review-stream` entry. |
| 3 | | Add :tab to switch tabs. | Section 5. The command row and `cargo xtask docgen`. Integration tests: `:tab code` and `:tab review` switch, and `:tab review` with no review fails with its message. Book: `keymap.md` (Review), generated `typable-cmd.md`. |
| 4 | | Leave the review out of the buffer lists. | Section 6. Integration tests: `:bn` skips the review from code and leaves it from the review, and `:bca` leaves it open. Book: `keymap.md` (Review). |
| 5 | | Name the review buffer. | Section 7. Integration test: the statusline's drawn row says `[review]`, read as `align-message`'s test reads it. |

Phase 1 is the riskiest, and everything else stands on it. Phases 4 and 5 don't depend on each other.

## Questions

Each one changes the plan. The plan assumes the first answer.

1. **The name.** `tab` (assumed), after Vim's tabpages. "Space" reads as Space mode (`Space b`, the book's "Space mode"). Other options: `layout` or `screen`.
2. **`:q` on the code tab's last view with a review open.** Quit Helix (assumed), as it does now that the review is a buffer. Or switch to the review tab, as Vim does when a tab's last window closes.
3. **`:q` on the review tab's last view.** Close the review (assumed). Or hide the tab and keep the review, so `:review` brings it back as it was.
4. **Gotos, pickers, and `:e` in the review tab.** They open there, and `Ctrl-o` comes back (assumed, as now). Or what they open moves to the code tab, like `Enter`.
   Moving needs a check after each key, since a goto's answer arrives later.
5. **The statusline.** The `[review]` name says where you are (assumed). A tab element waits for the git client, whose tab may show ordinary files. Or add a `tab` element now.
6. **`:bca` and `:bco`.** They leave the review open (assumed), or close it like any buffer.

## Not in this plan

- Tabs you make yourself, a tab line, and default keys.
- The git client.
- Moving a goto's answer to the code tab, unless question 4 asks for it.

## Found
