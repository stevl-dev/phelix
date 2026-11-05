# Using Helix

For a full interactive introduction to Helix, refer to the
[tutor](https://github.com/helix-editor/helix/blob/master/runtime/tutor) which
can be accessed via the command `phx --tutor` or `:tutor`.

> 💡 Currently, not all functionality is fully documented, please refer to the
> [key mappings](./keymap.md) list.

## Modes

Helix is a modal editor, meaning it has different modes for different tasks. The main modes are:

* [Normal mode](./keymap.md#normal-mode): For navigation and editing commands. This is the default mode.
* [Insert mode](./keymap.md#insert-mode): For typing text directly into the document. Access by typing `i` in normal mode.
* [Select/extend mode](./keymap.md#select--extend-mode): For making selections and performing operations on them. Access by typing `v` in normal mode.

## Buffers

Buffers are in-memory representations of files. You can have multiple buffers open at once. Use [pickers](./pickers.md) or commands like `:buffer-next` and `:buffer-previous` to open buffers or switch between them.

## Selection-first editing

Inspired by [Kakoune](http://kakoune.org/), Helix follows the `selection → action` model. This means that whatever you are going to act on (a word, a paragraph, a line, etc.) is selected first and the action itself (delete, change, yank, etc.) comes second. A cursor is simply a single width selection.

## Multiple selections

Also inspired by Kakoune, multiple selections are a core mode of interaction in Helix. For example, the standard way of replacing multiple instances of a word is to first select all instances (so there is one selection per instance) and then use the change action (`c`) to edit them all at the same time.

## Motions

Motions are commands that move the cursor or modify selections. They're used for navigation and text manipulation. Examples include `w` to move to the next word, or `f` to find a character. See the [Movement](./keymap.md#movement) section of the keymap for more motions.

## Recovering unsaved work

If the editor crashes with unsaved buffers, or a signal ends it (`SIGTERM`, `SIGINT`, or `SIGHUP`, which closing its terminal sends), it writes a copy of each one before it exits, and prints where on the terminal. When the terminal is gone, there's nowhere to print, but the copies are still written. Either way, the next start says where they are, once for each crash. The copies go in a directory of their own for each crash, under `recovery` in the data directory: `~/.local/share/phelix/recovery/<time>-<pid>/` on Linux and macOS, or `%AppData%\phelix\recovery\` on Windows. The data directory falls back to Helix's as the [config directory](./configuration.md) does.

Each copy is a plain UTF-8 file, named after the buffer's path with each `/` (and on Windows each `\` and `:`) replaced by `%`, as in `%home%user%src%main.rs`. A buffer with no path is written as `scratch-<id>`. A path too long for one file name keeps its end behind a hash of the whole path. On Unix, only you can read the copies.

Nothing reads the copies back or deletes them. To recover your work, open a copy, take what you need, and remove the directory.
