# Helix fork: crash recovery

Snapshot: `main` at `f01713111`, on upstream `master` at `ba40e547`.
Goes in `fork/crash-recovery.md`. Feature slug: `crash-recovery`.

Status: approved 2026-11-05.

## Bottom line

- When `phx` goes down with unsaved buffers, it writes their text to plain files under the data dir, and the next start says where they are.
- It covers every way down where the editor still runs code: a panic on the event loop, SIGTERM, SIGINT, SIGHUP, and losing the terminal. SIGHUP is what closing the terminal sends, and today it kills the process on the spot.
- It doesn't cover SIGKILL, the OOM killer, or power loss. Only periodic snapshots would, and they're not in this plan (decision 1).
- **How:** the event loop runs under `catch_unwind`. After a panic, `Application` still owns the `Editor`, so it writes the buffers and then lets the panic continue. A panic hook can't do this, because it can only reach statics.
- **Biggest risk:** phase 1. It reads documents whose edit a panic interrupted, and `is_modified` can lie about them.
- Size: 4 commits.

## Today

- On a panic, termina's hook restores the terminal and prints the message (`helix-tui/src/backend/termina.rs:231`), and every unsaved buffer is lost.
- SIGTERM and SIGINT restore the terminal and quit (`helix-term/src/application.rs:581`), which upstream's #7236 added in 2023. Unsaved buffers are dropped without a word.
- SIGHUP isn't among the registered signals (`application.rs:239`), so the default action kills the process.
- An error from the terminal's input stream panics: `handle_terminal_events` calls `event.unwrap()` (`application.rs:722`).

## What upstream did

- Issue #290, "Panic handler, that saves unsaved work to temporary files", has been open since 2021. Its thread settled three things:
  - The write should be "the dumbest, hardest-to-screw-up thing possible": plain files that any program opens (cessen).
  - "A panic handler only has access to statics", so it can't reach the documents. "It seems catching the panic may actually work tough" (pascalkuthe, 2023-09-16).
  - The maintainers hoped that persistent state, #401, would cover it. Its PRs were all closed without merging: #9143 (persistent state) and #5608 and #9154 (persistent undo).
- Nothing upstream conflicts with this plan, because there is no recovery code to replace.
  The backup that atomic save makes (`helix-view/src/document.rs:1100`) protects the file during a write, not unsaved text.
- Open PR #16396 is a live case. A stale document highlight panics on the next edit, "which takes the editor down with unsaved buffers".
  The fork has the same code: `helix-term/src/handlers/document_highlight.rs` has no version check. The fix for that is separate from this plan.

## What's written

- Every document whose `is_modified()` is true, and every document whose text differs from its file on disk (decision 8).
  After a panic, `is_modified` can say false for an edited buffer (see Risks), so it isn't trusted alone.
  The text is compared in the form a save would write. A file that can't be read counts as different.
  A scratch buffer is written if it's modified or not empty.
  A false positive writes one extra copy, which costs nothing. A false negative loses work.
- One dir per crash: `<data_dir>/recovery/<unix seconds>-<pid>/`. Two crashes, or two editors, never write into the same dir.
  The write makes `recovery/` and its parents if they're missing, as on a fresh install.
- One file per buffer, named after the buffer's full path with each separator and `:` replaced by `%`, as vim's `undodir` does: `%home%stevl%src%main.rs`.
  A name longer than 255 bytes becomes `<hash>-<tail>`: 16 hex digits of a hash of the full path, then the name's last 200 bytes, cut on a character boundary (decision 4).
  A scratch buffer is written as `scratch-<doc id>`.
- Plain UTF-8, with the buffer's own line endings, whatever the file's encoding was (decision 5).
- On Unix, the dir is mode `0700` and the files `0600`, since buffers can hold secrets.
- Each buffer is written on its own, so one failed write doesn't stop the rest. Failed writes are listed along with the written ones.
- Nothing deletes recovery files. The human removes them after taking what they need (decision 7).

## When

| Event | Today | After |
| --- | --- | --- |
| A panic on the event loop | The hook restores the terminal and prints the panic. Buffers are lost. | The same, then the buffers are written, a line on stderr names the dir, and the panic continues with exit code 101. |
| SIGTERM, SIGINT | The terminal is restored and the editor quits. Buffers are lost. | The buffers are written, the terminal is restored, a line on stderr names the dir, and the editor quits. |
| SIGHUP, or an error from the terminal's input | SIGHUP: default action, the process dies. An input error: a panic. | A hangup. The buffers are written, and the editor quits with exit code 129 without writing to the terminal, which is gone. |
| The next start | Nothing. | If `recovery/` holds a crash's dir that no start has named yet, the status line names it, and the dir is marked as seen (decisions 2 and 3). |
| SIGKILL, OOM, power loss | Buffers are lost. | Unchanged (decision 1). |
| A panic in a background task | Tokio keeps the editor running, but termina's hook has already reset the terminal. | Unchanged. Not in this plan. |

## How

- `helix-view/src/recovery.rs` writes an `Editor`'s buffers into a dir it's given, picking them as "What's written" says, and returns what it wrote and what failed.
  It uses plain `std::fs`, since the runtime may be going down when it runs. It's core code: it owns no buffers, tabs, state, or commands.
- In `Application::event_loop_until_idle` (`application.rs:313`), the loop runs under `AssertUnwindSafe(..).catch_unwind()`.
  On a panic it writes the buffers, prints the dir, and calls `std::panic::resume_unwind`.
  Both `run` and the test harness go through this function.
  The hook prints the panic message when the panic happens, so it prints once, before the line about the dir.
  `TerminaBackend`'s `Drop` skips its reset while `thread::panicking()` (`termina.rs:642`), so the resumed unwind leaves the terminal as the hook left it.
- `AssertUnwindSafe` is the claim this plan rests on: after a panic, each document's text can still be read.
  Ropey keeps a rope valid through its own edits; cessen fuzzed that for #290.
  The write only reads text and paths, and takes no lock that a panicking frame could hold.
  It doesn't trust the history, which a panic can leave empty (see Risks).
- `Application::new` takes the recovery dir, as it already takes the workspace trust.
  `phx/src/main.rs` passes `data_dir().join("recovery")`. The test harness passes a temp dir per app, so tests never write into the human's data dir.
- `handle_signals` (`application.rs:527`) writes before it returns `false` on SIGTERM, SIGINT, and SIGHUP. SIGHUP joins the registered signals.
  On SIGTERM and SIGINT it restores the terminal and then prints the dir.
- A hangup is SIGHUP or an error from the input stream, which today's `event.unwrap()` turns into a panic.
  Closing the terminal can deliver either one first, so both take the same path.
  `Application` records the hangup. `run` (`application.rs:1334`) then skips `restore_term`, whose `unwrap` in `handle_signals` would panic and whose `?` in `run` would hand `main` an error to print, and `eprintln!` panics when stderr is gone.
  `run` returns exit code 129, as a shell reports a death by SIGHUP. Nothing writes to stderr or the terminal after a hangup.
- At startup, `Application::new` lists the crash dirs in `recovery/` that hold no `.seen` file, names them in the status line, and writes an empty `.seen` into each.
  No buffer's file is named `.seen`: a path's name starts with `%` or a drive letter, a long one with its hash, and a scratch buffer's with `scratch-`.
  If writing `.seen` fails, the next start names that dir again, which is the safe way to fail.

## Plan

Each phase is one commit that builds and passes its tests, and marks itself done here.

| # | Status | Commit | Work |
| --- | --- | --- | --- |
| 0 | Done | Add the crash recovery plan. | This file. |
| 1 | | Write unsaved buffers to a recovery dir when the event loop panics. | `helix-view/src/recovery.rs`. The catch in `event_loop_until_idle`. The recovery dir passed through `Application::new` from `phx/src/main.rs`, with a temp dir from `helix-term/src/test_support.rs`. A unit test of the write into a temp dir: a modified file, an unmodified one that isn't written, an edited one whose history was reset so `is_modified` says false, a scratch buffer, two files with the same name in different dirs, a path whose name is too long, a missing `recovery/`, and the modes. An integration test: edit a buffer, dispatch a job that panics, and run the loop under `catch_unwind`. The panic still reaches the test, and the dir holds the edited text. A "Recovering unsaved work" section in `book/src/usage.md`. A `crash-recovery` entry in `FORK.md`. |
| 2 | | Write unsaved buffers on SIGTERM, SIGINT, and a hangup. | `handle_signals`, with SIGHUP registered. An input error handled as a hangup. `run` skipping the terminal after a hangup. A table-driven integration test after an edit, with SIGTERM, SIGINT, SIGHUP, and an input error: the loop stops and the dir holds the text. With nothing modified, no dir is made. A test that `run` returns 129 after a hangup. The book section and the `FORK.md` entry. |
| 3 | | Say at startup where unsaved work from a crash is. | `Application::new` checks the recovery dir, sets the status line, and marks what it named as seen. A unit test of the check: a missing dir, an empty one, one holding a new crash's dir, one holding only seen ones, and one holding both, where only the new one is named. An integration test: an app built with a new crash's dir shows the message, and a second app built on the same dir shows none. The book section. |

Phase 1 goes first because it's the riskiest: it decides whether the write can trust a document after a panic.
Phase 2 reuses the write. Phase 3 is the only way the human hears about a hangup, because stderr went with the terminal.

### Risks

- `is_modified` takes the history out of a `Cell` and puts it back (`document.rs:1846`).
  `undo_redo_impl` (`document.rs:1701-1708`) holds the history out of its `Cell` while `apply_impl` runs a full edit, which can panic.
  The `Cell` is then left with a default history at revision 0. A file never saved since it was opened has `last_saved_revision` 0, so it reads as unmodified.
  The comparison with the file on disk covers this. Phase 1's unit test resets a history to show it.
- A panic in the middle of `Transaction::apply` can leave a rope with half of an edit applied. The copy then holds that text: it's still the human's work, and it isn't the file.
- After a hangup, `close()` still runs: it finishes jobs, flushes writes, and stops the language servers. None of that writes to the terminal, but a job that does would fail. The buffers are already written by then.

### Manual verify, sketched

The recovery dir is `<data_dir>/recovery`, and `data_dir` is `~/.local/share/phelix`, or `~/.local/share/helix` when only that one exists (`helix-loader/src/lib.rs:137`).

- Phase 1: no key panics on purpose, so the integration test is what covers the panic path. Phase 2's check exercises the same write.
- Phase 2: open a file and type something without saving. From another shell, run `pkill -HUP -x phx`. `ls <recovery dir>/*/` lists the file, and it holds the typed text. Do the same with `pkill -TERM -x phx`: the shell gets its prompt back with a line naming the dir. Then close the terminal window instead, which may arrive as SIGHUP or as an input error: either way the dir holds the text.
- Phase 3: after phase 2's check, start `phx`. The status line names the crash's dir. Quit and start again, and no message appears, though the dir is still there.

## Decisions

Answered by the human:

1. No periodic snapshots, which would cover SIGKILL, the OOM killer, and power loss as vim's swap files do.
   They write to disk on a timer while you type, and are a feature of their own. Upstream's `auto-save.after-delay` already exists for anyone who wants their files written as they go.
2. A startup notice when there's unsaved work from a crash, as phase 3. Without it, a hangup leaves no trace on screen.
3. The notice names each crash once. It names only the crash dirs no start has named yet, and marks each one as seen. Showing the notice on every start until the files are removed was the other way.
4. Left to the plan: a name too long for the file system becomes `<hash>-<tail>`, the hash of the full path and the name's last 200 bytes.
   The other way was numbered files with an `index` that maps each one to its path. A cut name stays readable in `ls`, and there's no index to read.

Accepted as proposed:

5. Plain UTF-8 with the buffer's line endings, regardless of the file's encoding. It's the dumbest form that any program opens.
6. Vim-style names, one dir per crash. A path that holds `%` can map to the same name as another path, as in vim. Two such buffers in one crash are rare enough to accept.
7. No `:recover` command, and nothing deletes the files. To restore, open the copy and take what you need from it.
8. Modified buffers are written, and so is any buffer whose text differs from its file on disk.
9. Modes `0700` and `0600` on Unix.

## Not in this plan

- Periodic snapshots or swap files (decision 1).
- Persistent undo or session state (upstream's #401).
- Panics in background tasks, which leave the editor running on a terminal that termina's hook has reset.
- Panics outside `event_loop_until_idle`: the first `render` in `event_loop` (`application.rs:304`) and `close()` after the loop.
- The stale-highlight panic from #16396. It's a fix of its own.
- A restore command.

## Found
