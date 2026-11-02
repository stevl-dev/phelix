# Configuration

To override global configuration parameters, create a `config.toml` file located in your config directory:

- Linux and Mac: `~/.config/phelix/config.toml`
- Windows: `%AppData%\phelix\config.toml`

If that directory doesn't exist but Helix's does (`~/.config/helix`, or
`%AppData%\helix` on Windows), Phelix reads Helix's instead. The data directory,
which holds [workspace trust](./workspace-trust.md) grants, falls back the same
way. To switch, move the directory, for example
`mv ~/.config/helix ~/.config/phelix`. Don't create an empty one: an empty
`phelix` directory wins over a full `helix` one. `hx --health` shows the config
file in use.

> 💡 You can easily open the config file by typing `:config-open` within Helix normal mode.

Example config:

```toml
theme = "onedark"

[editor]
line-number = "relative"
mouse = false

[editor.cursor-shape]
insert = "bar"
normal = "block"
select = "underline"

[editor.file-picker]
hidden = false
```

You can use a custom configuration file by specifying it with the `-c` or
`--config` command line argument, for example `hx -c path/to/custom-config.toml`.
You can reload the config file by issuing the `:config-reload` command. Alternatively, on Unix operating systems, you can reload it by sending the USR1
signal to the Helix process, such as by using the command `pkill -USR1 hx`.

Finally, you can have a `config.toml` and a `languages.toml` local to a project by putting it under a `.helix` directory in your repository.
Its settings will be merged with the configuration directory and the built-in configuration.

## `[review]` Section

Which of the theme's diff styles the [review](./keymap.md#review) draws. The colors come from the theme's `diff` [scopes](./themes.md#syntax-highlighting).

| Key | Description | Default |
|--|--|---------|
| `row-background` | Tint added and removed rows with `diff.plus.view` and `diff.minus.view`. When `false`, removed rows keep their syntax colors, and only the gutter's `-` marks them | `true` |
| `word-background` | Mark changed words with `diff.delta.view.text` | `true` |

`:config-reload` reads the table again after you change it. It's a table of its own, beside `[editor]`, so `:set` and `:toggle` don't reach it.
