# Phelix

Phelix is a personal hard fork of [Helix](https://github.com/helix-editor/helix),
the Kakoune and Neovim inspired modal editor written in Rust.

It isn't affiliated with the Helix project, and the Helix maintainers don't
support it. Report Helix's own bugs to Helix only when they also happen in
upstream Helix.

Phelix doesn't rebase onto Helix. Helix's changes come over by cherry-pick,
when they're worth it, so the two drift apart over time.

# What it adds

- `:review` shows every changed file's hunks in one buffer, against `HEAD`, or
  with `unstaged` or `staged` against the index. Edits to the hunks go to the
  files as you type.
- Tabs, each a layout with its own splits and focus: `code`, where the editor
  starts, and `review`.
- The changed-files picker (`Space g`) lists unstaged, staged, or all changes,
  and `Ctrl-g` cycles between them.
- `[keys.picker]` in `config.toml` rebinds the picker's keys.
- `:yank-diagnostics` copies every diagnostic in the buffer as a compiler
  report.
- The message below the statusline lines up with the statusline's text.

[`FORK.md`](./FORK.md) lists every change, with where its code and docs are.
The [book](./book/src/SUMMARY.md) documents the editor, the changes included.

# Building

You need the [Rust toolchain](https://www.rust-lang.org/tools/install), Git,
and a C++14 compiler for the tree-sitter grammars.

```sh
git clone https://github.com/stevl-dev/phelix
cd phelix
cargo install --path phx --locked
```

That installs the `phx` binary. It needs the `runtime` directory from the
checkout. Point it there with `PHELIX_RUNTIME`, or link it into the config
directory, and check the result with `phx --health`:

```sh
export PHELIX_RUNTIME=~/src/phelix/runtime
```

[Building from source](./book/src/building-from-source.md) has the other
options.

# Configuration

Phelix reads `~/.config/phelix/config.toml`, or `%AppData%\phelix\config.toml`
on Windows. While that directory doesn't exist and Helix's `~/.config/helix`
does, it reads Helix's instead. The data directory falls back the same way.

To give Phelix a config of its own, move or copy Helix's directory:

```sh
mv ~/.config/helix ~/.config/phelix
```

Don't create an empty one first: an empty `phelix` directory wins over a full
`helix` one. Workspace-local `.helix/` directories still work.

The [configuration](./book/src/configuration.md) page has the rest.

# License

Phelix is distributed under the [Mozilla Public License 2.0](./LICENSE), like
Helix. Nearly all of it is the work of the Helix contributors.
