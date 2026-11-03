# Helix fork: rename to Phelix

Snapshot: `main` at `afb38609`, on upstream `master` at `ba40e547`.
Goes in `fork/rename.md`. Feature slug: `rename`.

Status: done 2026-11-03. Phases 0–5 are committed, and the GitHub repo is `stevl-dev/phelix`.

## Bottom line

- The fork gets a name of its own, Phelix, so nobody takes the repo, the binary, or the config for upstream Helix.
- The binary becomes `phx`. Its crate, `hx`, becomes `phx` with it.
- Config, cache, and data move to `phelix` dirs. Config and data fall back to Helix's dirs until the human moves them, so nothing breaks on the day it lands.
- The `HELIX_*` environment variables become `PHELIX_*`.
- The README says what Phelix is: a personal hard fork of Helix, not affiliated with it, under Helix's MPL-2.0. It stops borrowing upstream's logo, badges, and links.
- The GitHub repo becomes `stevl-dev/phelix` and stays a GitHub fork of `helix-editor/helix`.
- What stays Helix's: the `helix-*` crates, workspace-local `.helix/`, `runtime/`, the names editors send to language servers and debug adapters, and the paths named for the fork (`FORK.md`, `fork/`, `tools/fork/`).
- Biggest risk: phase 1, since it changes where the editor reads the human's config and writes its data. It goes first.
- Size: 6 commits.

## What carries the name

| What | Today | After | Phase |
| --- | --- | --- | --- |
| Config dir | `~/.config/helix` | `~/.config/phelix`, else `~/.config/helix` if only that exists | 1 |
| Data dir (workspace trust grants) | `~/.local/share/helix` | `~/.local/share/phelix`, else `~/.local/share/helix` if only that exists | 1 |
| Cache dir and log | `~/.cache/helix/helix.log` | `~/.cache/phelix/phelix.log`, no fallback | 1 |
| Runtime env vars | `HELIX_RUNTIME`, `HELIX_LOG_LEVEL` | `PHELIX_RUNTIME`, `PHELIX_LOG_LEVEL`, no fallback | 2 |
| Build env vars | `HELIX_DEFAULT_RUNTIME`, `HELIX_DISABLE_AUTO_GRAMMAR_BUILD`, `HELIX_NIX_BUILD_REV` | `PHELIX_*` | 2 |
| Binary and its crate | `hx`, `hx/` | `phx`, `phx/` | 3 |
| Shell completions, launcher | `contrib/completion/hx.*`, `contrib/hx_launcher.sh` | `phx.*`, `phx_launcher.sh` | 3 |
| Packages and desktop entry | deb `helix`, `/usr/lib/helix`, `Helix.desktop`, `helix.png`, nix `helix` | `phelix` throughout | 4 |
| Version and help lines | `helix 25.7.1 (…)`, `hx 25.7.1` | `phelix 25.7.1 (…)` | 5 |
| README, Cargo `repository` and `homepage`, `book.toml` | upstream's | the fork's | 5 |
| GitHub repo | `stevl-dev/helix` | `stevl-dev/phelix` | 5 |

### The fallback

`config_dir()` and `data_dir()` in `helix-loader/src/lib.rs` pick once per run, each on its own:

1. `<base>/phelix`, if it exists.
2. `<base>/helix`, if it exists.
3. `<base>/phelix` otherwise, so a new install gets the new name.

Moving a dir is the whole migration: `mv ~/.config/helix ~/.config/phelix`.
An empty `phelix` dir wins over a full `helix` one, so the book says to move the dir, not to create it.
`phx --health` names the config file it read, so the human can see which dir won.

The fallback is the human's choice, not a shim kept for code. It stays until the human asks to drop it.

## Plan

Each phase is one commit that builds and passes its tests, and marks itself done here.

| # | Status | Commit | Work |
| --- | --- | --- | --- |
| 0 | Done | Add the rename plan. | This file. |
| 1 | Done | Give the editor its own config, cache, and data dirs, falling back to Helix's. | `config_dir`, `cache_dir`, `data_dir`, and `default_log_file` in `helix-loader/src/lib.rs`, with the rule above held in a `OnceLock`. A table-driven unit test of the choice in a temp dir: neither exists, only `helix`, only `phelix`, both. The book pages that name the dirs (`configuration.md`, `themes.md`, `languages.md`, `workspace-trust.md`, and the rest a grep finds), with a line on moving the dir. `agents/VERIFY.md`'s config path. A new `rename` entry in `FORK.md`. |
| 2 | Done | Rename the HELIX_ environment variables to PHELIX_. | `HELIX_RUNTIME` and `HELIX_DEFAULT_RUNTIME` in `helix-loader/src/lib.rs` and its doc comment, `HELIX_NIX_BUILD_REV` in `helix-loader/build.rs`, `HELIX_DISABLE_AUTO_GRAMMAR_BUILD` in `helix-term/build.rs`, `HELIX_LOG_LEVEL` in `helix-term/src/application.rs`. `contrib/hx_launcher.sh`, `default.nix`, and the book pages that name them (`building-from-source.md` and the rest a grep finds). |
| 3 | Done | Rename the binary and its crate to phx. | `hx/` becomes `phx/`: package, `[[bin]]`, `default-run`, the workspace's `members`, `default-members`, and `package.hx.opt-level`, and `xtask`'s dependency and `hx::FEATURES`. `USAGE:` in `main.rs`, the `'hx --grammar fetch'` hints in `helix-loader/src/grammar.rs`. `contrib/completion/hx.*` become `phx.*` and complete `phx`. `contrib/hx_launcher.sh` becomes `phx_launcher.sh`. The binary's paths in the deb metadata, `Exec` and `TryExec` in the desktop entry, and the nix files. Every book page that runs `hx`. `AGENTS.md`, `FORK.md` (Build, `feature-hooks`), `agents/HELIX.md`, `agents/REVIEW.md`, `agents/VERIFY.md`. |
| 4 | Done | Name the packages and the desktop entry phelix. | The deb metadata: `name`, `/usr/lib/phelix`, `/usr/share/doc/phelix`, and the launcher's paths. `contrib/Helix.desktop` becomes `Phelix.desktop`, with `Name=Phelix` and `Icon=phelix`. `contrib/helix.png` becomes `phelix.png`, the same image. `contrib/Helix.appdata.xml` goes. `default.nix`, `flake.nix`, and `grammars.nix`: package, overlay, and derivation names. |
| 5 | Done | Name the editor Phelix in its text, README, and links. | `--version` and `--help` print `phelix`. The workspace-trust prompt in `helix-term/src/handlers/workspace_trust.rs` says Phelix. The README: what Phelix is, that it isn't affiliated with Helix, what it adds (from `FORK.md`), how to build it and where its config lives, credit to Helix and its MPL-2.0 license. Helix's logo, badges, Matrix, and website links go. Cargo `repository` and `homepage`, and the fork's author added to `authors` after upstream's. `book.toml`: title, `git-repository-url`, `edit-url-template` on `main`, and no `cname`. The book's `title-page.md`, `install.md` (build from source only), and `building-from-source.md`. `package-managers.md` and its `SUMMARY.md` line go. `.github/ISSUE_TEMPLATE/` goes, as `fork/hard-fork.md` left it for this plan. The `gh repo set-default` line in `AGENTS.md` and `FORK.md` names `stevl-dev/phelix`, which also fixes the stale `stepan-ogorodnikov/helix`. Then, at the human's ask: `gh repo rename phelix`, `git remote set-url origin https://github.com/stevl-dev/phelix.git`, `gh repo set-default stevl-dev/phelix`, then `land.sh`. |

Phase 1 changes where user data lives, so it goes first.
Phase 3 is the largest diff, but it's mechanical, and the compiler and `land.sh --check` find what it misses.
The README goes last, so everything it says is already true.
The GitHub rename doesn't depend on any phase. It's in phase 5 so the repo name and the links that name it change together. GitHub redirects the old URLs.

After phase 3 lands, `cargo install --path phx --locked` installs `phx` beside the old `hx` in `~/.cargo/bin`. `cargo uninstall hx` removes the old one.

## Decisions (2026-11-02)

The human's answers:

1. Name: Phelix, "personal Helix".
2. Binary: `phx`.
3. Config, cache, and data: dirs of their own, falling back to Helix's.
4. GitHub: stay a fork of `helix-editor/helix`. Detaching would buy the banner, the contribution graph, and `gh` defaulting to the fork, and the human wants none of those enough. The `gh repo set-default` rule stays.
5. `FORK.md`, `fork/`, and `tools/fork/` keep their names.

The human approved the plan with the answer it assumed to each of these:

6. The environment variables get no fallback. A stale `HELIX_RUNTIME` aimed at an installed upstream's runtime would load queries that don't match the fork's grammars.
7. The cache dir gets no fallback. It holds only the log.
8. Config and data fall back each on their own, by whether the dir exists.
9. The crate `hx` is renamed with the binary. It holds only the binary and the feature list.
10. The names sent over protocols stay Helix's: the LSP `clientInfo.name` `helix` in `helix-lsp/src/client.rs`, the DAP `client_id` `hx` in `helix-dap/src/client.rs`, and `hostInfo = "helix"` in `languages.toml`. A server may key its behavior on them.
11. The book renames what a reader types or looks for: the binary, dirs, env vars, packages, and install. Prose that calls the editor Helix stays. Renaming it would touch every page and every book change a cherry-pick brings.
12. The desktop entry keeps Helix's icon, renamed so it can't overwrite an installed upstream's. The README drops the logo.
13. The version stays the workspace's `25.7.1`.

## Not in this plan

- The `helix-*` crates and the `helix_*` paths in code.
- Workspace-local `.helix/`. Other people's projects keep working.
- `runtime/`: the tutor, themes, queries, and `languages.toml`'s links. Cherry-picks bring most of their changes.
- `docs/` and `CHANGELOG.md`, which are upstream's.
- A logo of the fork's own.
- A version scheme of the fork's own.
- Detaching from the fork network on GitHub.
- The name of the local checkout's directory. The human can rename it any time.

## Found

- Phase 1: `languages.toml` gives the global ignore file its `git-ignore` type by the glob `.config/helix/ignore`. It gained `.config/phelix/ignore`, and keeps Helix's for the fallback.
  `building-from-source.md` links the runtime into `~/.config/phelix/runtime`. Making that dir ends the config fallback, which is what `configuration.md`'s note on moving warns about.
  `helix-loader` also builds an `hx-loader` binary, which upstream's release CI used. Phase 3 deals with it.
- Phase 2: `docs/CONTRIBUTING.md` names `HELIX_LOG_LEVEL`, and `agents/HELIX.md` sends readers there for it, so it took the new name, though this plan leaves `docs/` alone otherwise.
  `helix-core/tests/indent.rs` sets the runtime var for its test, and took the new name.
  `hx --health` with `PHELIX_RUNTIME` set lists that dir among the runtime dirs, and lists `~/.config/helix/runtime` where the human has only Helix's config dir.
- Phase 3: `hx-loader` went in a commit of its own before this phase, since nothing ran it once upstream's release CI was gone.
  `docs/CONTRIBUTING.md`'s `hx -v` and `docs/architecture.md`'s crate table took the new name, for the same reason as phase 2.
  `install.md`'s pre-built binaries section and `package-managers.md` still say `hx`, since they describe upstream's builds. Phase 5 rewrites the one and deletes the other.
  `--help` reads its first line from the crate's name, so it says `phx 25.07.1` until phase 5 names the editor.
- Phase 4: `grammars.nix` keeps its `helix-tree-sitter-*` names, and `flake.nix` keeps the `helix.cachix.org` substituter. Grammar derivations with upstream's names and inputs can still come prebuilt from upstream's cache.
  `default.nix` named the package after the binary's crate, which phase 3 made `phx`. It names it `phelix` outright now.
  `contrib/helix-256p.ico` is linked into the Windows binary and never installed, so it kept its name.
  `docs/releases.md` still tells upstream's release process to edit the deleted `Helix.appdata.xml`. The whole page is upstream's process, which this plan leaves alone.
  Neither nix nor `cargo-deb` is installed here, so the nix files and the deb metadata were checked by reading them. `desktop-file-validate` passes on `Phelix.desktop`.
- Phase 5: the README no longer shows `logo_dark.svg`, `logo_light.svg`, or `screenshot.png`, and nothing else used them, so they went. `logo.svg` stays, since `default.nix` installs it as the icon.
  `--help` joined the authors with `:`, as `CARGO_PKG_AUTHORS` does, once there were two. It joins them with `, `.
  The fork's author went into `authors` by name only, without an email.
  `title-page.md` lost its tiles for upstream's stable and `master` docs, which the fork doesn't publish. `version.js` stays, and finds no tiles to mark.
  `mdbook` isn't installed here, so the book wasn't built. `package-managers.md` had one link to it, from `install.md`, which phase 5 rewrote.
  The GitHub rename ran on 2026-11-03, after one more commit that replaced Helix's tagline, "A post-modern text editor.", with "Personal Helix: an opinionated fork of the Helix editor." in `--help`, the Cargo and nix descriptions, and the repo's description.
