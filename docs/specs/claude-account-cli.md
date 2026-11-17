# Claude Account CLI

Type: FEATURE

## Problem

Developers who have more than one personal Claude Code account on a Mac need a safe, quick way to choose which account future Claude Code sessions use. Repeatedly signing out and in makes it easy to use the wrong account and disrupts current work.

## User stories

- As a developer, I can save several named Claude Code account profiles so that I can reuse them without repeatedly configuring each one.
- As a developer, I can see and switch the active profile so that future Claude Code sessions use the intended account.
- As a developer, I can remove or reauthenticate a profile safely when its credentials are no longer wanted or valid.

## Acceptance criteria

- AC1: On macOS, a developer can add a uniquely named profile through Claude Code's normal interactive sign-in flow; the first profile becomes active.
- AC2: A developer can list profiles, see the active profile, rename a profile, switch profiles, reauthenticate a profile, and remove a non-active profile.
- AC3: Each profile has its own directory inside `~/.claude`, and switching selects that directory through `CLAUDE_CONFIG_DIR` for future Claude Code launches without interrupting existing sessions.
- AC4: The tool does not copy or display credentials; Claude Code manages authentication within each profile configuration, and normal or JSON output never exposes secrets.
- AC5: List and status provide both human-readable output and JSON output suitable for scripts.
- AC6: The tool explains failures without changing profiles when Claude Code is unavailable, a name is invalid or duplicated, a profile is unknown, or credentials cannot be accessed.

## Detailed behaviour

The command is `claude-account`. Profiles have a user-chosen, case-insensitively unique label and display the signed-in email after it is available. The active profile is global for the local computer. `~/Library/Application Support/claude-account/profiles.json` is the source of truth for managed profiles, recording each profile's label, configuration directory, and email, plus the active profile. Each profile owns a dedicated directory under `~/.claude`, such as `~/.claude/work` or `~/.claude/personal`.

Adding and reauthenticating use Claude Code's normal interactive sign-in. During reauthentication, the active profile is not changed if the sign-in fails. When the tool is first used and an existing Claude Code sign-in is present, it offers to import that sign-in as a named profile. Existing Claude Code installation and settings remain unchanged unless the user chooses to import or activate a profile.

Switching changes the selected profile and exposes its directory as `CLAUDE_CONFIG_DIR` for future launches only; it does not copy profile credentials into the shared `~/.claude` root. The tool provides a shell-friendly command for applying the selected directory. It informs the user if current Claude Code sessions may continue with their existing account. A profile with rejected or expired credentials must be reauthenticated before it can be activated. A currently active profile cannot be removed; the user must switch to another profile first. Removal asks for confirmation and supports a non-interactive force option.

Empty state output explains how to add or import a first profile. Loading operations show clear progress. Existing signed-in profile directories are reconciled into the registry on startup. Sign-in cancellation, unavailable Claude Code, and missing or invalid account state show actionable error messages and leave the selected profile unchanged.

## Access and permissions

This release is for one local macOS developer managing their own accounts. The local user must be able to use Claude Code and macOS Keychain.

## Business rules

- Labels are required and unique without case sensitivity.
- The first successfully added profile is made active automatically.
- Default terminal output is human-readable; `list` and `status` support JSON.
- The CLI does not store secret authentication material; profile metadata contains no credentials.
- Claude Code must be installed and available before adding, switching, or reauthenticating a profile.

## Scope

- In scope: macOS profile management, normal Claude Code sign-in, global switching, registry reconciliation, and script-readable list/status output.
- Out of scope: shared-machine use, Linux or Windows, project-specific profile selection, profile export/import, notes, automatic Claude Code installation, and changing active running sessions.
- Must stay unchanged: the user's existing Claude Code installation, non-authentication settings, and any session already running.
