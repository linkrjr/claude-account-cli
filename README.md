# claude-account-cli

`claude-account` is a macOS Rust CLI for keeping named Claude Code account profiles and switching the account used by future Claude Code launches.

## Install

```sh
cargo install --path .
```

Claude Code must already be installed and available as `claude`.

## Usage

```sh
# On first use, save an existing Claude Code sign-in
claude-account import-current personal

# Or create a profile through Claude Code's normal sign-in flow
claude-account add work

claude-account list
claude-account switch personal
claude-account status --json
claude-account reauth work
claude-account rename work client-a
claude-account remove client-a
```

Each profile owns its Claude Code configuration directory:

```text
~/.claude/personal/
~/.claude/work/
```

The CLI launches account setup and reauthentication with `CLAUDE_CONFIG_DIR` set to that profile directory. It also records the selected profile globally. To make every normal `claude` launch respect that selection, add this Zsh function to `~/.zshrc`:

```zsh
claude() {
  local config_dir
  config_dir="$(claude-account env --value)" || return
  CLAUDE_CONFIG_DIR="$config_dir" command claude "$@"
}
```

Open a new shell after adding it. `claude-account switch work` then changes the directory that the wrapper uses, without copying credentials into the shared `~/.claude` root.

`claude-account` does not copy or display authentication secrets; Claude Code continues to manage its own authentication inside each profile configuration. `~/Library/Application Support/claude-account/profiles.json` is the source of truth for every managed profile: its label, configuration directory, email address, and the current active profile. Existing signed-in profile directories are reconciled into this registry on startup.

Switching does not affect Claude Code sessions that are already running.
