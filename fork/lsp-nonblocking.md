# Helix fork: LSP requests off the event loop

Snapshot: `main` at `c94c9abce`, on upstream `master` at `ba40e547`.
Goes in `fork/lsp-nonblocking.md`. Feature slug: `lsp-nonblocking`.

Status: approved 2026-11-05.

## Bottom line

- A code action picked from the menu, and a rename (upstream's and the review's), stop waiting for the server on the event loop.
  Their edits apply when the answer arrives. If the buffer changed in the meantime, the edit is dropped and the status line says so.
- The transport answers `workspace/configuration` and `workspace/workspaceFolders` itself. That ends the known deadlock at the two places that still wait: accepting a completion, and the file operations.
- The first phase adds a fake language server for the integration tests. Upstream has none, so none of this is tested upstream.
- **Biggest risk:** phase 2, which changes the shape of upstream's code-action `Action`.
- Size: 5 commits.

## The problem

The event loop also answers the server's requests: `workspace/configuration` is answered at `helix-term/src/application.rs:1124`.
Say the loop blocks waiting for a response, and the server asks the editor something before it answers. The server now waits on the editor and the editor waits on the server, until the request times out. The timeout is 20 s by default (`helix-core/src/syntax/config.rs:653`).
Meanwhile no key is handled and nothing is drawn. A server that's just slow freezes the editor the same way.

typescript-language-server 5.1.0 and 5.1.1 hit this on every rename and on some completion accepts (upstream #14738, #14846).
5.1.2 works around it on the server's side. Any other server that asks for its configuration in the middle of a request hits it too.

## What upstream did

- In #14738 (open, labeled `C-bug`), the-mikedavis wrote on 2025-11-21: "anywhere that we block on a response from a language server is problematic. That blocks the main thread which is also responsible for responding to requests from language servers. […] Rename requests might be simpler […] But refactoring completion to avoid blocking will be hard."
- Upstream has been taking the blocks out one at a time:
  - formatting, #14873 (2025-11);
  - document links, #15440 (2026-04);
  - code actions on save, `9fc0e104a` (2026-06).
- PR #16336 (opened 2026-10-02, open, no review yet) sends the rename as a job, and drops its edit when the document has changed. Phase 3 takes the same approach.
- In the other direction, #15448 (2026-05) added `willCreate` and `willDelete`, which block the way `willRename` (#8584) does.
- #16336 and #16396 both say they left their tests out because the integration tests have no language server.

Nothing here reverts upstream work.
Each site that becomes async does what upstream did for formatting and for code actions on save: it remembers the document's version with the request, and drops an answer that's stale.

## The sites

| Site | Origin | In this plan |
| --- | --- | --- |
| `rename_symbol`, `helix-term/src/commands/lsp.rs:1246` | upstream | Phase 3: a job. |
| The review's rename, `helix-review/src/term/lsp.rs:90` | fork | Phase 3: a job. |
| A code action from the menu, `helix-view/src/action.rs:76` | upstream, `bc27d0113` (2026-06) | Phase 2: a job. |
| Accepting a completion, `helix-term/src/ui/completion.rs:410` | upstream (2021) | Still waits. Phase 4 ends its deadlock (decision 1). |
| `willRename`, `willCreate`, `willDelete`, `helix-view/src/editor.rs:1680`, `1729`, `1775` | upstream, #8584 and #15448 | Still wait. Phase 4 ends their deadlock. |
| DAP (`commands/dap.rs`, `handlers/dap.rs`, `typed.rs:2205`) and shell pipes (`commands.rs:6502`) | upstream | Not in this plan. |

Why the file operations still wait:

- `apply_workspace_edit` itself calls them, for a workspace edit's create, rename, and delete operations (`helix-view/src/handlers/lsp.rs:248` to `277`).
- The spec has the client apply the server's edits before it touches the file. So making them async would make applying any workspace edit async, renames included.
- They only run on `:move` and on edits that carry file operations.

Why accepting a completion still waits, unless decision 1 says otherwise:

- The item goes in when you accept it. Its extra edits, such as auto-imports, can come with the resolve.
- To apply them later, they'd have to be mapped through everything typed since.
- The review takes the extra edits aside when you accept (`helix-review/src/view/complete.rs:64`), so it would need a late path too.
- The accept only waits when the item wasn't already resolved on idle. That happens 150 ms after the item is selected (`helix-term/src/handlers/completion/resolve.rs`).

## How

### The fake server (phase 1)

- A `fake-lsp` binary in `helix-term`, behind the `integration` feature through `required-features`, so `cargo install` never builds it. It speaks JSON-RPC on stdio.
- Each test scripts it through the language config:
  - the capabilities it announces;
  - an answer for each method;
  - before it answers a method, an optional request of its own that it sends and waits on. That reproduces the deadlock;
  - an optional notification it waits for before answering. That forces "the buffer changed first" without sleeping.
- `test_support` gets a helper that builds a language config running the fake with a short `timeout`, so a deadlock fails a test in a second, not twenty.
- helix-term's tests find the binary through `CARGO_BIN_EXE_fake-lsp`.
  Cargo sets that only for a package's own binaries, so the review's tests look in the target directory, one level above their own test binary (`target/<profile>/`, next to `deps/`).
  It's there only when the same run builds helix-term's binaries. `cargo integration-test` does, through `--workspace`; `cargo test -p helix-review` doesn't. When it's missing, the helper panics and says to run `cargo integration-test`.
- The phase's own test exercises a request that's already async: `gd` jumps where the fake says.
- `agents/TESTING.md` gets a section on it.

### Code actions (phase 2)

- `Action` gets a second kind of function: one that returns a future, whose output is applied on the event loop.
- `Action::lsp` uses it when the action needs resolving. An action the server sent complete still runs at once.
  `Action::lsp` is the only code that builds an `Action`, so nothing else changes shape.
- The menu (`helix-term/src/commands/lsp.rs:673`) runs the future through `job::dispatch`.
- `Action::lsp` doesn't know the document. `code_action` records the document and its version when it opens the menu, and the menu's job checks them before it applies the output, as code actions on save do (`resolve_and_apply_code_actions_of_kind`, `commands/lsp.rs`).
- The resolved action's command still runs after its edit, as it does now. When the edit is dropped as stale, its command is dropped with it.
- The doc comment on `Action` names a spell checker as another source of actions. There's none, so the comment goes.
- Tests:
  - The server asks for its configuration before it answers `codeAction/resolve`: the edit applies. Before this phase, that test times out.
  - The server waits for a `didChange`, and the test types after picking the action: the edit is dropped, and the status line says so.

### Renames (phase 3)

- Upstream's rename: as in #16336. It sends the rename as a job, and checks the version of the document the rename started in.
- The review's rename does the same. It already refuses a file that changed before the request goes out, and the callback checks again.
  The callback takes the revisions for the review's undo step right before it applies the edit, not when the request goes out.
- Tests:
  - The server asks for its configuration first: the rename applies. Before this phase, that test times out.
  - Typing before the answer arrives drops the rename, with its message.
  - The same pair for the review, in `helix-review/tests/`.

### Configuration and workspace folders (phase 4)

- `Transport::start` gets the client's config, which is fixed when the client starts (`helix-lsp/src/client.rs:64`), and the client's workspace folders, which the client puts behind an `Arc` to share them.
  Its receive task answers `workspace/configuration` and `workspace/workspaceFolders` from them.
  The arms at `application.rs:1124` and `1121` go.
- It replies the way it already does to a request that comes after shutdown: through `inject_tx` (`helix-lsp/src/transport.rs:226`).
- The answers are the same as now: the event loop reads the same fields.
- This works because `block_on` (`futures_executor`) blocks only the thread it runs on, and the receive task runs on another worker of the multi-thread runtime.
  `phx` uses `#[tokio::main]`, and every integration test uses `flavor = "multi_thread"`. A test on a current-thread runtime would still deadlock.
- The event loop still answers the server's other requests. Two of them could deadlock the same way, and they stay:
  - `window/workDoneProgress/create` (`application.rs:1082`) starts the spinner, which is editor state.
  - `workspace/applyEdit` (`application.rs:1096`) edits documents.
- Tests: accept a completion whose item isn't resolved yet, from a server that asks for its configuration before it answers `completionItem/resolve`. The item goes in with its extra edit. The same test again, with a server that asks for its workspace folders. Before this phase, both time out.

## Plan

Each phase is one commit that builds and passes its tests, and marks itself done here.

| # | Status | Commit | Work |
| --- | --- | --- | --- |
| 0 | Done | Add the plan for LSP requests off the event loop. | This file. |
| 1 | Done | Add a fake language server for the integration tests. | The `fake-lsp` binary and its script format. The helper in `helix-term/src/test_support.rs`. The `gd` test. The section in `agents/TESTING.md`. |
| 2 | | Resolve code actions from the menu without blocking. | `helix-view/src/action.rs` and the menu in `helix-term/src/commands/lsp.rs`. The two tests. |
| 3 | | Send rename requests without blocking. | `rename_symbol` in `helix-term/src/commands/lsp.rs` and `rename` in `helix-review/src/term/lsp.rs`. The two tests for each. |
| 4 | | Answer configuration and workspace folders in the transport. | `helix-lsp/src/transport.rs` and `client.rs`. The two arms in `application.rs` go. The two completion tests. |

Phase 1 comes first, because every later phase needs it for its tests.
Phase 2 is the riskiest: it changes the type that upstream's June refactor (`bc27d0113`) introduced, so it goes as early as the harness allows.
Phases 3 and 4 each start with a test that fails because of the deadlock.

No `FORK.md` entry: this is a fix. The fake server is documented in `agents/TESTING.md`.

### What changes for the user

- Before, a keystroke typed while a rename or a code action waited sat in the terminal and ran after the edit. Now it runs first, and the edit is dropped if that keystroke changed the buffer.
  With a responsive server that window lasts a few milliseconds.
  Upstream's formatting and code actions on save already behave this way.

### Manual verify, sketched

- Phase 3: with typescript-language-server 5.1.1 (the version with the deadlock), open a `.ts` file with `const x = 1; const y = x + x;`. Put the cursor on `x`, press `Space r`, type `z`, then `Enter`. All three `x` become `z` at once. Before, the editor froze for 20 s and said the request timed out.
- Phase 2: on a diagnostic that offers a quick fix needing resolve, `Space a`, `Enter`. The fix applies, and the cursor keeps moving while you wait.

## Decisions

Answered 2026-11-05:

1. Accepting a completion: (a).
   - (a) It keeps waiting. Phase 4 ends the known deadlock, and a slow server still freezes the accept, but only when the item wasn't resolved on idle.
   - (b) It waits at most about 1 s, then inserts the item without its extra edits and says so.
   - (c) It becomes fully async: the item goes in at once, and its extra edits are mapped through what you typed since, the review's deferred path included.
     The maintainers call this hard. It would be phase 2, as the riskiest.
2. Rename: written here, after #16336, not cherry-picked once upstream merges it. The review's rename is done here either way.

Assumed, and approved with the plan:

3. The file operations, DAP, and shell pipes are out of scope.
   So are `window/workDoneProgress/create` and `workspace/applyEdit`, which the event loop keeps answering. (Moving `workspace/workspaceFolders` into the transport was decided on 2026-11-05.)
4. A stale answer is dropped with a status-line error, not mapped through the edits.
5. The fake server is a binary in `helix-term` behind `integration`.

## Not in this plan

- Cancelling a request in flight, for example with `C-c`. the-mikedavis suggests it in #14738.
- Showing progress while a rename is in flight.
- Making the file operations async (see the sites above).
- DAP's blocking calls, and the shell pipe's.
- Answering `window/workDoneProgress/create` and `workspace/applyEdit` off the event loop.

## Found

Phase 1:

- One lookup finds `fake-lsp` for every crate's tests, helix-term's included: one level above the test binary. `CARGO_BIN_EXE_fake-lsp` would be set only while compiling helix-term's tests, and `test_support` is in helix-term's library, so it can't read it.
- A server is initialized before the event loop hears of it, and the loop sends `didOpen` only then. `wait_for_language_servers` runs the loop once more after the server is up.
- Key events win over job callbacks in the event loop's `select!`. A key that opens a menu from a server's answer has to go in a sequence of its own, or the keys after it run before the menu exists.
- A language with no grammar loads fine: the `fake` language only logs that its parser is missing.
