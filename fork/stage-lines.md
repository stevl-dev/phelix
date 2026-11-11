# Helix fork: stage and unstage lines in the review

Snapshot: `main` at `62f046673`.
Goes in `fork/stage-lines.md`. Feature slug: `stage-lines`, part of `review-stream`'s entry in `FORK.md`.

Status: approved 2026-11-11. Phases 0–1 are committed.

## Bottom line

- `:stage-line` and `:unstage-line` write the changed lines the selections cover in the review to the index, and leave the rest of each hunk as the index has it.
- They sit beside `:stage-hunk` and `:unstage-hunk`, which keep taking whole hunks from a cursor anywhere in one.
- They work in all three reviews: `all`, `unstaged`, and `staged`.
- A selection may cover unchanged rows, headers, and gaps. Only its removed and added rows count.
- Nothing new reaches git. The commands build the index's new text in memory and hand it to `set_index_text`, as the hunk commands do, so CRLF and clean filters, the executable bit, added and removed files, conflicts, and the trust gate behave as they do today.
- This reverses `fork/review-stream.md`'s ruling "Staging takes whole hunks".
- Biggest risk: the `all` review, whose rows show `HEAD` against the file while staging changes the index between them. Phase 1 has it.
- Size: 3 commits, about 250–350 lines plus tests, most of it in `helix-review/src/term/stage.rs`.

## What a row means

A removed row is a line of the review's base, and an added row a line of its source.

| Review | Removed row | Added row |
| --- | --- | --- |
| `all` | a line of `HEAD` | a line of the file |
| `unstaged` | a line of the index | a line of the file |
| `staged` | a line of `HEAD` | a line of the index |

`:stage-line` takes rows that show an unstaged change, and `:unstage-line` rows that show a staged one.

| Review | Row | `:stage-line` | `:unstage-line` |
| --- | --- | --- | --- |
| `all` | removed | drops the line from the index, if the index still has it | puts the line back, if the index dropped it |
| `all` | added | adds the line, if the index lacks it | drops the line, if the index has it and `HEAD` doesn't |
| `unstaged` | removed | drops the line from the index | nothing |
| `unstaged` | added | adds the line | nothing |
| `staged` | removed | nothing | puts the line back |
| `staged` | added | nothing | drops the line |

When no selected row has anything to do, the command fails with the hunk commands' messages: `There are no unstaged changes under any selection`, or `staged`.
Afterwards the review reads its changes again, and the status says `Staged 2 lines` or `Unstaged 1 line`, counting the selected rows that took effect.

## How the index's new text is built

Each command has a diff of its own, read fresh when it runs:

- `:stage-line`: the index against the review's source, as `:stage-hunk`.
- `:unstage-line`: `HEAD` against the index, as `:unstage-hunk`. `HEAD`'s text of a renamed file comes from its old path, as the review reads it.

1. Every removed and added row of a file with a selected one is carried to a line of that diff.
   A removed row goes from the review's base to the diff's side through a diff of the two texts, and an added row from the review's source likewise.
   Where the two texts are the same, the line stays. A line inside a changed range is carried nowhere, and its row has nothing to do.
   This is what fills the table above, with no branch on the review's scope, and it also holds when the index changed since the review read it.
2. A hunk of the command's diff is touched when a selected row is carried into it.
3. In a touched hunk, a line stays as the index has it only when the review shows it as a removed or added row outside the selection. Every other line of the hunk goes with it.
4. The hunk's lines keep the review's order: the removed lines that stay, then the added ones.
   Staging only the `+` row of a `-`/`+` pair leaves the old line and the new one in the index, as `git add -p` does.

Rule 3 is what makes the `all` review work. An example: `HEAD` has `b`, the index has `B` staged, and the file has `B2`.
The `all` review shows `-b` and `+B2`. The index's `B` has no row, so staging `+B2` takes `B` out with it, and the index gets `B2`, not `B` and `B2`.
It also gives an invariant to test: selecting every row of a hunk does what the hunk command does.

A last line with no line ending that ends up above another line gets the file's line ending, so the two don't join.

`splice` in `stage.rs` grows the lines that stay, and the hunk commands pass none, so there is one function that builds the index's text.

## Plan

Each phase is one commit that builds and passes its tests, and marks itself done here.

| # | Status | Commit | Work |
| --- | --- | --- | --- |
| 0 | Done | Add the plan for staging lines in the review. | This file. |
| 1 | Done | Stage the changed lines the selections cover in the review. | `Stream::picked_lines` in `helix-review/src/view.rs`: for each file with a selected removed or added row, every such row's line and whether the selection covers it. In `term/stage.rs`: carrying a line through a diff, `splice` with the lines that stay, and `stage_line`. The `stage-line` row in `term.rs`. Unit tests in `stage.rs`, table-driven over three texts and a selection: a partly staged replacement, an untouched hunk left alone, every row selected equals the whole hunk, the `B`/`B2` case above, a line the index already has, and a last line with no line ending. A unit test of `picked_lines` in `view.rs`: a selection over a header, a gap, and unchanged rows gives only the changed ones. Integration tests in `helix-review/tests/test/review.rs`: one added line of a two-line hunk in an `unstaged` review, a removed and an added row in an `all` review of a file with staged and unstaged changes, and the message when the selection covers no unstaged change. `book/src/keymap.md` (Review), `book/src/generated/typable-cmd.md` from `cargo xtask docgen`, and `review-stream`'s entry in `FORK.md`. |
| 2 | Proposed | Unstage the changed lines the selections cover in the review. | `unstage_line` in `term/stage.rs`, on phase 1's pieces with `HEAD` and the index as the sides, and `HEAD`'s text read from a rename's old path. The `unstage-line` row in `term.rs`. Unit tests: a partly unstaged replacement, a removed line put back, and unstaging the last staged line of a file `HEAD` lacks removes it from the index. Integration tests: an added row in a `staged` review, a row in an `all` review, and the message in an `unstaged` review. The same book pages and `FORK.md` entry. |

Phase 1 has the carry through two diffs and rule 3, which is where a wrong line would reach the index, so it goes first.
Phase 2 swaps the sides and adds nothing of that kind.

After each phase, the paths to check in `phx` are: select one row of a hunk with `x`, run the command, and read the rows that are left and the status line.
I'll offer to run them, as `agents/VERIFY.md` says.

## Decisions (2026-11-11)

The human's answers:

1. A new pair of commands, `:stage-line` and `:unstage-line`. The hunk commands stay.
2. The `all` review is supported from the start.
3. Rows of a selection that aren't removed or added rows are ignored.

The human approved the plan with the answer it assumed to each of these:

4. A changed line the review can't show goes with its hunk (rule 3). The other choice, changing only the selected lines, puts `B` and `B2` in the index in the example above.
5. `:unstage-line` finds nothing in an `unstaged` review, and `:stage-line` nothing in a `staged` one, since neither shows that kind of change. `:unstage-hunk` does work in an `unstaged` review today, so the two differ there.
6. No default keys, as for the hunk commands. They're bound by name, like `[keys.normal.space] s = ":stage-line"`.
7. The status counts lines, not hunks: `Staged 2 lines`.

## Not in this plan

- `:review-restore` by line. It still puts back whole hunks.
- Staging part of a line.
- Moving `:stage-hunk` and `:unstage-hunk` onto the row carry. They keep picking hunks by source line.
- `:unstage-hunk` on a renamed file. It reads `HEAD`'s text at the new path, where `HEAD` has no file, so by the code it would unstage the whole file. This was read, not run. If a test shows it, the fix is a commit of its own.

## Risks

- Repeated lines, like a lone `}`. The review's diff and the command's diff can pair them differently. A selected row then has nothing to do, or a line with no row goes with its hunk. Only the `all` review has two diffs to disagree, and only where the index differs from both `HEAD` and the file.
- A file changed on disk since the review read it, with no buffer open. The review's source is stale, and its lines go to the index. `:stage-hunk` has the same gap today.

## Found

- Phase 1: there is no `Stream::picked_lines`. `Stream::picked` gives each file its removed and added rows' lines too, since a file has a hunk under the selection exactly when it has a changed row under it. So `write_picked` serves both commands, and takes how to word the count.
  `splice` takes the text for each range it replaces, as slices, and the line ending to put after a line that has none. The hunk commands hand it one slice per hunk.
  The line ending is the index's own, else the source's, else a line feed.
  `staged_lines` is the pure part: a file's rows and the index's text in, the new text and the count out. Its unit test builds the rows from a diff of the base and the source, as the review does.
  Staging a line can leave the review's text the same: an added row becomes an unchanged one with the same text. The integration tests wait on the status and the index, not on the rows.
