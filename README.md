# qpath

Quick Path — register, list, and maintain frequently used file and directory paths.

`qpath` keeps a registry of path abbreviations in TOML files, renders them through a small template language, drops entries whose paths do not exist, and prints the rest for shell and editor integration.  It is designed as a data source: shell widgets and editor commands read its output instead of maintaining their own path lists.

## Installation

```console
% cargo install qpath --locked
```

Or from a checkout:

```console
% cargo install --path . --locked
```

## Configuration

Definitions are loaded from, in this order:

```text
~/.config/qpath/paths.toml      # default target for editing commands
~/.config/qpath/paths.d/*.toml  # sorted by file name
```

The same abbreviation may be defined more than once; like `git config`, the last definition in load order wins (among entries that pass type and existence filtering).  This lets a later file under `paths.d/` override an entry from `paths.toml`.

Each `[[path]]` table defines one entry:

```toml
[[path]]
abbr = "gh"
path = "~/src/github.com/"
desc = "GitHub"

[[path]]
abbr = "i"
path = "~/.emacs.d/init.el"
type = "file"
```

`abbr` and `path` are required.  When `desc` is missing, the path itself is used as the description in output.  `type` is one of `directory` (`dir`, `d`), `file` (`f`), or `existent` (`exist`, `e`); when omitted, a path ending in `/` is a `directory` and anything else is `existent`.  A leading `~/` or `~user/` is expanded.  Entries whose rendered path does not exist, or does not match their type, are silently skipped.

### Templates

Paths are rendered with a Jinja2-like template language ([MiniJinja](https://github.com/mitsuhiko/minijinja)).  Built-in variables: `home`, `config_home`, `data_home`, `cache_home`, `state_home`, `os`, and `arch`.  The `*_home` variables are platform aware — on macOS `config_home` is `~/Library/Application Support`, elsewhere `${XDG_CONFIG_HOME:-~/.config}`.

Custom variables can be defined in a `[vars]` table in any definition file:

```toml
[vars]
emacs_dir = "~/.emacs.d/"

[[path]]
abbr = "ed"
path = "{{ emacs_dir }}"
desc = "Emacs"
```

Available filters:

- `glob` — Expand a glob pattern into an array of matching paths.  Brace alternations such as `{a,b}` are expanded first and each alternative is globbed independently; matches are sorted lexically within each alternative and concatenated.  A trailing `/` restricts matches to directories.
- `vsort` — Sort an array in version-fragment order, so `python3.9` sorts before `python3.10` and `v29.9` before `v29.10`.  Pipe `glob` through this to pick the newest versioned match with `last`.
- `shell` — Run a command with `/bin/sh -c` and return its standard output, trailing newlines stripped like `$(...)`.  `shell('zsh')` or another shell name changes the interpreter.  `cache_ttl=SECONDS` caches the output per command and shell under `~/.cache/qpath/shell/`; failures are never cached.
- MiniJinja built-ins such as `first`, `last`, `sort`, `reverse`, and `join`.

```toml
[[path]]
abbr = "sp"
path = "{{ '/opt/homebrew/lib/python3.[0-9]*/site-packages/' | glob | vsort | last }}"
desc = "Python site-packages"

[[path]]
abbr = "p"
path = "{{ '~/{.Debfile,.Brewfile}' | glob | first }}"
desc = "Package list"

[[path]]
abbr = "brew"
path = "{{ 'brew --prefix' | shell(cache_ttl=86400) }}/etc/"
desc = "Homebrew etc"

[[path]]
abbr = "zp"
path = "{{ 'print -r -- $HOME/zsh-project' | shell('zsh') }}"
desc = "zsh project"
```

## Usage

### Listing

```console
% qpath ls
as	~/Library/Application Support/	/Users/you/Library/Application Support/	~/Library/Application\ Support/
gh	GitHub	/Users/you/src/github.com/	~/src/github.com/
```

`qpath ls [--type TYPE] [--format tsv|json] [--expand]` lists entries as `abbr`, `desc`, `path`, `shell_path` TSV columns or a JSON array.  JSON output also includes `source`, the absolute path to the definition file that produced the entry.  `path` is the raw absolute path, ready to pass to file APIs.  `shell_path` is quoted for direct insertion into a shell command line, with a leading `~/` left unquoted so it stays expandable.  When `desc` is missing, the `~/`-shortened path is shown instead.  `--type` filters by what the path is on disk (default `existent`), and `--expand` makes `desc` and `shell_path` use absolute paths instead of shortening under `~/`.  `list` is an alias for `ls`.

`qpath show <abbr> [--type TYPE] [--format tsv|json] [--expand]` prints the single entry whose abbreviation matches exactly, in the same columns as `ls`, and errors if no such entry exists (or is filtered out by `--type`).

### Editing

```console
% qpath add gh ~/src/github.com/ --desc GitHub
% qpath update gh ~/src/gitlab.com/
% qpath rename gh hub
% qpath rm hub
% qpath format
```

Editing commands preserve comments and formatting, and keep entries sorted (`--sort-by abbr|path`).  `qpath add` targets `~/.config/qpath/paths.toml` by default; `--file` selects another definition file.  It always appends an entry, so the same abbreviation may appear more than once; `--overwrite` instead replaces the last existing entry in the target file (or appends if there is none), preserving fields not given on the command line.  Without `--file`, `qpath update <abbr> [path]`, `qpath rename <abbr> <new-abbr>`, and `qpath rm <abbr>` edit the file containing the last matching definition in load order, the same entry that wins at load time.  With `--file`, they edit the last matching entry in that file instead.  The path for `update` is optional, so it can change only `--desc` or `--type`.  `qpath format` re-sorts a file edited by hand and tidies its whitespace (trailing spaces, repeated blank lines).  `remove` is an alias for `rm`, and `fmt` for `format`.

### Cache

```console
% qpath cache clear [shell]
```

Removes cached data under `~/.cache/qpath/`; with `shell`, only the shell filter's command cache.

### zsh integration

```sh
insert-qpath() {
  local sel
  sel=$(qpath ls --type directory | fzf --delimiter='\t' --with-nth=1,2 --bind 'one:accept' --query='^' | cut -f4) || return
  LBUFFER+=$sel
}
zle -N insert-qpath
bindkey '^Xq' insert-qpath
```

## Author

Copyright (c) 2026 Akinori Musha.

Licensed under the MIT license.  See `LICENSE` for details.

Visit the [GitHub Repository](https://github.com/knu/qpath) for the latest information.
