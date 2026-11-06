# Verifying changes

Tests cover what they can reach.
For the rest, the agent offers to check the change in `phx` itself, with the human's permission, when it can read the result from the screen.
What it can't check, it hands to the human as a short brief.

## Agents must not

- Run `phx` without the human's permission for that check. A yes covers the check it was given for, not later ones.
- Claim a picker, a keymap, or anything drawn on screen works because the tests passed.
- Ask the human to "try the picker" without saying what to press and what should happen.

## Agents must

1. Run the tests from `agents/TESTING.md` (Running) for the crates the change touched.
2. For a change to what the user sees or presses, write its paths, as the Manual verify block below lays them out.
3. Sort the paths. A path the agent can check from the screen's text, it offers to run. A path it can't, it leaves in the Manual verify block. Say which is which, and why a path can't be checked.
4. Run the offered paths only after the human says yes, and report what each showed. A path that showed something other than it said is a finding: say whether the change or the path was wrong.
5. For a change the user can't see, like a moved body or a type alias, write `Manual verify: none — <why>` instead.

## What the agent can check

It reads the screen as text, so it can check what text shows: the buffer, the bufferline, the statusline and its messages, a picker's rows and count, a popup's text, the tab and the cursor's place.

It can't check, so the human does:

- Colors, styles, and the cursor's shape. Text capture drops them.
- The mouse, the system clipboard, terminal focus, and a terminal's own keys.
- Timing and flicker, and what needs a server or a tool that isn't installed.

## Running a check

- Make the fixture with a script in the scratchpad that deletes and rebuilds it, so each path can start fresh. Never point `phx` at the fork's own checkout or at the human's files.
- Check that the installed `phx` has the change: `phx --version` names its commit.
- Run `phx` in a detached tmux session of a fixed size, as `tmux new-session -d -s <name> -x 120 -y 30 -c <fixture> 'phx <file>'`.
- Send keys with `tmux send-keys`, and read the screen with `tmux capture-pane -p`.
- Wait for text on the screen, in a loop under `timeout`, never for a fixed time. Escape lands after a delay, so wait for the mode in the statusline before the next key.
- A path that changes the fixture, like a commit, starts from a fresh one, and so does each path that needs a clean editor.
- Kill the session when done. It drops unsaved changes, which in a fixture is what you want.
- In zsh, a word that starts with `=`, such as an `=====` separator, expands to a command's path. Use `-----`.

## Manual verify block

Put it in the message that hands the work back, with the paths the human checks.
Usually 3–7 paths, not a tour of the editor.

````markdown
## Manual verify

- Build: `cargo install --path phx --locked`
- Config, if the change needs one, in `~/.config/phelix/config.toml`, or `~/.config/helix/config.toml` until it's moved:
  ```toml
  [keys.picker]
  C-n = "move_next"
  ```
- Paths:
  - <starting state> → <keys> → <what you should see>
  - <one cancel or negative path, when the change adds a guard>
- Skip if: <when a path doesn't apply>
````

How to write a path:

- Start from a state the human can reach: a file to open, or a repository with something staged.
  Give the shell commands that set it up, when that isn't obvious, or the script that builds the fixture.
- Name the keys, like `Space g`, `C-g`, or `:yank-diagnostics<ret>`, not the command's function.
- Name what's on screen: the picker's status label, the message in the status line, the text after `"+p`.
- Check what a key does against the book before saying what it shows. A goto from the review, for example, opens in the code tab.
- Prefer a path that would fail if the bug were real.
