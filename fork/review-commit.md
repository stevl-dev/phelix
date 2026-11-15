# Helix fork: review a commit

Snapshot: `main` at `ed65461ff`.
Goes in `fork/review-commit.md`. Feature slug: `review-commit`, part of `review-stream`'s entry in `FORK.md`.

Status: phases 0–1 are committed.

## Bottom line

- **Build:** a picker that lists the commits `HEAD` reaches, and a review of the one you pick: its message, then every file it changed, in the review's tab.
- **The review already shows two texts per file** and doesn't care where they come from. A commit's are its parent's and its own.
- **A commit's review is read-only**, as a staged review is. It takes no edits, stages nothing, and asks no language server.
- **There is still one review.** A commit replaces the changes it showed, and is refused while a file of those has unsaved changes.
- **No new dependency.** `gix` already diffs trees and walks history. Naming a commit by `HEAD~2` or a branch turns on its `revision` feature.
- **The core changes nowhere.** The picker and the commands are in `helix-review`, the git reads in `helix-vcs`.
- **Biggest risk:** the message's rows belong to no file, and every row of the stream has belonged to one until now. Phase 4 has it.
- **Size:** 7 commits. About 250 lines in `helix-vcs` and 350–450 in `helix-review`, plus tests. This is an estimate from reading the code.

## 1. The view

```
           commit ed65461ff3c08d1b7a6e5f4c3b2a1908f7e6d5c4
           Author: Ada Lovelace <ada@example.com>
           Date:   2026-11-15 14:02 +0100

               Show the review's file list when the tab is wide.

               A narrow tab has no room for it.

 helix-review/src/term.rs  modified ───────────────────────────
           ⋯ 40 unchanged lines
  41  41     fn render(&self, area: Rect) {
  42      -      let width = area.width - 2;
      42 +      let width = area.width.saturating_sub(2);
```

- The message's rows come first: the hash, the author, the author's date, a `Merge:` line with the parents' short hashes for a merge, then the message, indented four columns as `git show` has it.
- They have no line numbers and no sign in the gutter, and no theme scope of their own.
- Below them the stream is what it is today: a header per file, its hunks, and its gaps.
- The file list beside it shows the commit's files, with their status and counts.
- The buffer still goes by `[review]`. The status line says `Reviewing ed65461ff: Show the review's file list…` when it opens.

## 2. Design

### 2.1 Reading a commit (`helix-vcs`, phase 1)

A new module, `helix-vcs/src/git/commit.rs`, with a facade in `helix-vcs/src/commit.rs`, as the index has.

- `commit(dir, rev, trust_full)` gives the commit, which is its hash, author, date, parents, and message, and its changed files, each a `FileChange` with both sides' bytes.
- `rev` is anything `git rev-parse` takes for one commit. That needs `gix`'s `revision` feature.
- The paths are absolute, under the work tree, as the status walk's are. A bare repository is refused.

| Case | What it gives |
| --- | --- |
| A merge | The changes against its first parent. `git show`'s combined diff is empty for most merges. |
| A root commit | Every file as added. |
| A parent the clone doesn't have, as in a shallow clone | An error that says so. |
| A rename | `FileChange::Renamed`, found with the status walk's settings: half the lines, no copies. A copy is an added file. |
| A symlink | Its target, as text. |
| A submodule, or a file whose mode alone changed | Two equal sides, which the review shows as a header with `no text changes`. |
| A binary file | Its bytes. The review already finds it binary and shows a header. |
| Line endings and filters | Both sides go through the worktree conversion, as `get_diff_base` does, so `autocrlf` leaves no line changed. An untrusted workspace runs no filter program, as today. |

### 2.2 A scope of the review's own (phase 2)

`Stream::scope` is a `ChangeView`, the changed-file picker's list: `all`, `unstaged`, or `staged`.

- The review gets its own `Scope`: a `ChangeView`, or a commit.
- Ten places ask `scope == ChangeView::Staged`, and each means "the sources aren't the files". They ask the scope that instead, so a commit's review answers as a staged one.
- `File::staged` in `term.rs` is the same question and takes the same name.
- This phase changes no behavior. The review's tests pass as they are.

### 2.3 A commit in the review (phase 3)

`:review-commit [rev]` shows a commit's changes, `HEAD`'s with no argument. It reads the repository the open review shows, or else the one that holds the current file, as `:review` does.

**It replaces what the review showed.**

- While a file the review shows has unsaved changes, it's refused, with `:bc`'s message, and the review stays.
- Otherwise the review's buffer takes the commit's rows, in place, so its views, the file list, and `:review-context`'s count stay.
- The buffers the review loaded in the background close, and its undo log empties, as when the review closes.
- The cursor goes to the top. A second commit replaces the first the same way.
- `:review all`, `unstaged`, or `staged` replaces a commit with those changes. `:review` alone switches to the open review, whatever it shows.

**It's read-only.** The statusline shows it, as for a staged review.

| In a commit's review | What happens |
| --- | --- |
| An edit | Put back: `A commit's review is read-only. ret opens the file.` |
| `u`, `U` | The same message. |
| `:w` | Writes nothing, as in a staged review. |
| `:stage-hunk`, `:stage-line`, `:unstage-hunk`, `:unstage-line`, `:review-restore` | Refused: `A commit's review shows the commit, not the files.` |
| Goto, hover, code action, rename, completion, signature help | Refused with that message. The file's language server knows the file as it is now, not as the commit has it. |
| Diagnostics | None. |
| `:reload`, `:reload-all` | Reads the commit again. |
| `Enter` on a file's row | Opens the working tree's file in the code tab at the row's line number, or its last line. A file the working tree doesn't have is an error that names it. |
| `Enter` on a gap, the hunk motions, `gn`, `gp`, `:review-context`, `:review-sidebar` | As today. |

- An open buffer's text is never a side of a commit's review, as it is never one of a staged review.
- A file's encoding is detected from its bytes.
- A commit that changes no file opens no review: the status line says `ed65461ff changes no files`, and an open review stays. This is `:review`'s rule that no changes means no review.

### 2.4 The message's rows (phase 4)

- `Row` gets a kind for a line of the message, which belongs to no file. `Row::file` answers with an `Option`, and its ten callers decide what no file means.
- The rows are laid out once, from the commit the scope holds, before the first file's header.
- An edit to them is put back, as any edit is in this review. `Enter` on one does nothing.
- With the cursor on one, the file list marks no file.
- `gn` from the message goes to the first file's header, and `gp` from that header stays.
- A reload keeps the cursor on its row of the message, by the row's line.

### 2.5 Listing commits (`helix-vcs`, phase 5)

- `commits(dir, trust_full, f)` walks every commit `HEAD` reaches, newest first by commit date, and hands each to `f` until `f` returns `false`, as `for_each_changed_file` does.
- A merge is one commit in the list, and its parents' histories both follow.
- A repository with no commit yet is an error.

### 2.6 The picker (phase 6)

`:review-log` opens it, on the repository `:review-commit` would read.

| Column | Shows |
| --- | --- |
| `hash` | The short hash |
| `date` | The author's date, as `2026-11-15` |
| `author` | The author's name |
| `subject` | The message's first line. The filter starts here. |

- The walk runs off the main thread and adds commits as it finds them. Closing the picker stops it.
- `Enter` opens the commit as `:review-commit` does. The split keys do the same, since the review has one tab.
- It has no preview. A picker previews a file or a buffer, and the message is in the review.
- `last_picker` (`Space '`) opens it again with its filter and its cursor, for the next commit.
- No default key. Bind it by name, like `[keys.normal.space] l = ":review-log"`.

## 3. Left out

- The history of one file, or of a branch other than `HEAD`'s.
- A list that stays open beside the review, as the file list does.
- A range of commits, or one commit against another.
- A merge's changes against its second parent.
- Carrying `Enter`'s line from the commit's text to the file's text of today.
- A limit on a commit's size. A commit of thousands of files is laid out whole, as a working tree with that many changes is today.

## Plan

Each phase is one commit that builds and passes its tests, and marks itself done here.

| # | Status | Commit | Work |
| --- | --- | --- | --- |
| 0 | Done | Add the plan for reviewing a commit. | This file. |
| 1 | Done | Read a commit's changes from git. | 2.1: `git/commit.rs`, the facade, and `gix`'s `revision` feature. Unit tests in `helix-vcs/src/git/test.rs`, over repositories the tests build: one commit that modifies, adds, deletes, and renames a file, with both sides of each; a root commit; a merge against its first parent; a file whose mode alone changed; a rev that names nothing, with its message; a CRLF file under `autocrlf` whose commit changes one line. |
| 2 | | Give the review a scope of its own. | 2.2. No behavior changes, so no new test. |
| 3 | | Show a commit's changes in the review. | 2.3: `:review-commit`, the replace rule, and the refusals. Integration tests in `helix-review/tests/test/review/commit.rs`: the rows of a commit that isn't `HEAD`, with a working tree that differs from it; an edit is put back with the message; `:stage-hunk` and a goto are refused; a review with an unsaved file refuses the commit and stays; `:review all` afterwards shows the working tree's changes; `Enter` opens the file, and names one the working tree lacks; a commit with no changes leaves the open review. Book, generated docs, and `FORK.md`. |
| 4 | | Show the commit's message above its files. | 2.4: the row kind, the layout, the gutter, and the list's mark. Unit tests in `view.rs`: the layout's text and rows for a commit with a body, with none, and for a merge. Integration tests: the cursor starts on the first row, `gn` goes to the first header, an edit to the message is put back, and the list marks no file there. Book and `FORK.md`. |
| 5 | | List a repository's commits. | 2.5. Unit tests in `git/test.rs`: the order over a history with a merge, each commit once; a walk that `f` stops; a repository with no commit. |
| 6 | | Pick a commit to review from a list. | 2.6: `:review-log`. Integration tests: `Enter` on the first row shows `HEAD`'s commit, and a filter on a subject opens that commit. Book, generated docs, and `FORK.md`. |

Phase 1 comes first because everything reads through it.
Phase 4 has the risk, and goes as early as it can: it needs a commit in the review to show.
The picker is last, since `:review-commit` already reaches every commit by name.

After phases 3, 4, and 6, the paths to check in `phx` are: open a commit of this repository, read its rows against `git show`, try an edit, and go back to `:review`.

## What the work found

- **Phase 1.** `helix_vcs::commit::read` is the reader. The date is a string the reader formats, since `helix-review` has no date library and `gix` does.
  The tree diff lists directories too, which the reader skips. A shallow clone still names the parent it lacks, so the error for it has a test.
  The rename settings are one function now, `renames` in `git.rs`, which the status walk and the reader share.
