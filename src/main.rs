use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Parser)]
#[command(
    name = "claude-account",
    about = "Manage local Claude Code account profiles"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Sign in with Claude Code and save the resulting account under LABEL.
    Add { label: String },
    /// Save the account currently signed in to Claude Code under LABEL.
    ImportCurrent { label: String },
    /// List saved account profiles.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Show the active account profile.
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Print the selected profile's Claude configuration directory for shell integration.
    Env {
        /// Print only the directory path, without shell syntax.
        #[arg(long)]
        value: bool,
    },
    /// Use LABEL for future Claude Code launches.
    Switch { label: String },
    /// Rename a saved profile.
    Rename { label: String, new_label: String },
    /// Sign in again and refresh Claude Code's account state for LABEL.
    Reauth { label: String },
    /// Remove a saved, non-active profile.
    Remove {
        label: String,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    active: Option<String>,
    profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Profile {
    label: String,
    email: Option<String>,
    #[serde(default)]
    config_dir: PathBuf,
}

#[derive(Serialize)]
struct Status<'a> {
    selected: Option<&'a Profile>,
    config_dir: Option<PathBuf>,
    config_dir_exists: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut app = App::load()?;
    match cli.command {
        None => app.print_help(),
        Some(Commands::Add { label }) => app.add(&label),
        Some(Commands::ImportCurrent { label }) => app.import_current(&label),
        Some(Commands::List { json }) => app.list(json),
        Some(Commands::Status { json }) => app.status(json),
        Some(Commands::Env { value }) => app.env(value),
        Some(Commands::Switch { label }) => app.switch(&label),
        Some(Commands::Rename { label, new_label }) => app.rename(&label, &new_label),
        Some(Commands::Reauth { label }) => app.reauth(&label),
        Some(Commands::Remove { label, force }) => app.remove(&label, force),
    }
}

struct App {
    metadata_path: PathBuf,
    claude_root: PathBuf,
    store: Store,
}

impl App {
    fn load() -> Result<Self> {
        let home = home_dir()?;
        let metadata_path = home.join("Library/Application Support/claude-account/profiles.json");
        let claude_root = home.join(".claude");
        let mut store = read_store(&metadata_path)?;
        let mut migrated = false;
        for profile in &mut store.profiles {
            if profile.config_dir.as_os_str().is_empty() {
                profile.config_dir = claude_root.join(&profile.label);
                migrated = true;
            }
        }
        let known_labels = store
            .profiles
            .iter()
            .map(|profile| profile.label.to_ascii_lowercase())
            .collect::<Vec<_>>();
        for profile in discover_profiles(&claude_root, &known_labels)? {
            store.profiles.push(profile);
            migrated = true;
        }
        if migrated {
            write_atomically(&metadata_path, &serde_json::to_string_pretty(&store)?)?;
        }
        Ok(Self {
            metadata_path,
            claude_root,
            store,
        })
    }

    fn print_help(&self) -> Result<()> {
        if self.store.profiles.is_empty() && self.legacy_config_path().exists() {
            println!(
                "A Claude Code sign-in was found. Save it with:\n  claude-account import-current <label>"
            );
        } else if self.store.profiles.is_empty() {
            println!("No profiles saved. Add one with:\n  claude-account add <label>");
        } else {
            println!("Use `claude-account --help` to see available commands.");
        }
        Ok(())
    }

    fn add(&mut self, label: &str) -> Result<()> {
        self.require_new_label(label)?;
        ensure_claude_available()?;
        println!(
            "Claude Code will open now. Complete its normal sign-in flow, then exit Claude Code to save this profile."
        );
        let status = self.start_claude_for(label)?;
        if !status.success() {
            bail!("Claude Code exited unsuccessfully; profile was not saved");
        }
        self.capture_profile(label, self.store.profiles.is_empty())
    }

    fn import_current(&mut self, label: &str) -> Result<()> {
        self.require_new_label(label)?;
        ensure_claude_available()?;
        let config = fs::read_to_string(self.legacy_config_path())
            .with_context(|| "no Claude Code account state found to import")?;
        validate_profile_config(&config)?;
        write_atomically(&self.profile_config_path(label), &config)?;
        self.capture_profile(label, self.store.profiles.is_empty())
    }

    fn capture_profile(&mut self, label: &str, activate: bool) -> Result<()> {
        let config = fs::read_to_string(self.profile_config_path(label))
            .with_context(|| "no Claude Code account state found; finish signing in first")?;
        validate_profile_config(&config)?;
        self.store.profiles.push(Profile {
            label: label.to_string(),
            email: extract_account_email(&config),
            config_dir: self.claude_root.join(label),
        });
        if activate {
            self.store.active = Some(label.to_string());
        }
        self.save()?;
        println!(
            "Saved profile `{label}`{}.",
            if activate { " and made it active" } else { "" }
        );
        Ok(())
    }

    fn list(&self, json: bool) -> Result<()> {
        let mut profiles = self.store.profiles.clone();
        profiles.sort_by_key(|profile| profile.label.to_ascii_lowercase());
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&Store {
                    active: self.store.active.clone(),
                    profiles,
                })?
            );
            return Ok(());
        }
        if profiles.is_empty() {
            println!("No profiles saved. Add one with `claude-account add <label>`.");
            return Ok(());
        }
        for profile in &profiles {
            let marker = if self.store.active.as_deref() == Some(&profile.label) {
                "*"
            } else {
                " "
            };
            let email = profile.email.as_deref().unwrap_or("email unavailable");
            println!(
                "{marker} {} ({email}) — {}",
                profile.label,
                profile.config_dir.display()
            );
        }
        Ok(())
    }

    fn status(&self, json: bool) -> Result<()> {
        let active = self
            .store
            .active
            .as_ref()
            .and_then(|label| self.profile(label));
        let config_dir = self
            .store
            .active
            .as_deref()
            .and_then(|label| self.profile(label))
            .map(|profile| profile.config_dir.clone());
        let status = Status {
            selected: active,
            config_dir: config_dir.clone(),
            config_dir_exists: config_dir.as_ref().is_some_and(|path| path.is_dir()),
        };
        if json {
            println!("{}", serde_json::to_string_pretty(&status)?);
            return Ok(());
        }
        match active {
            Some(profile) => println!(
                "Selected account: {} ({})",
                profile.label,
                profile.email.as_deref().unwrap_or("email unavailable")
            ),
            None => println!("No selected account."),
        }
        if let Some(directory) = config_dir {
            println!(
                "Claude config directory: {}{}",
                directory.display(),
                if status.config_dir_exists {
                    ""
                } else {
                    " (missing)"
                }
            );
        }
        Ok(())
    }

    fn env(&self, value: bool) -> Result<()> {
        let label = self.store.active.as_deref().context("no active profile")?;
        let directory = self
            .profile(label)
            .context("active profile is missing")?
            .config_dir
            .clone();
        if !directory.is_dir() {
            bail!(
                "profile directory for `{label}` is missing; run `claude-account reauth {label}`"
            );
        }
        if value {
            println!("{}", directory.display());
        } else {
            println!("export CLAUDE_CONFIG_DIR={}", shell_quote(&directory));
        }
        Ok(())
    }

    fn switch(&mut self, label: &str) -> Result<()> {
        ensure_claude_available()?;
        let profile = self.profile(label).cloned().context("unknown profile")?;
        let config = fs::read_to_string(self.profile_config_path(&profile.label))
            .with_context(|| format!("profile directory for `{label}` is missing; run `claude-account reauth {label}`"))?;
        validate_profile_config(&config)
            .context("saved account state is invalid; run `claude-account reauth <label>`")?;
        self.store.active = Some(profile.label.clone());
        self.save()?;
        if claude_is_running() {
            eprintln!("Note: existing Claude Code sessions continue using their current account.");
        }
        println!(
            "Active profile: {} ({})",
            profile.label,
            profile.email.as_deref().unwrap_or("email unavailable")
        );
        println!(
            "Future Claude Code launches should use: {}",
            profile.config_dir.display()
        );
        Ok(())
    }

    fn rename(&mut self, label: &str, new_label: &str) -> Result<()> {
        validate_label(new_label)?;
        let index = self.index_of(label).context("unknown profile")?;
        if self.index_of(new_label).is_some() {
            bail!("a profile named `{new_label}` already exists");
        }
        let old_directory = self.store.profiles[index].config_dir.clone();
        let new_directory = self.claude_root.join(new_label);
        if new_directory.exists() {
            bail!(
                "profile directory `{}` already exists",
                new_directory.display()
            );
        }
        fs::rename(&old_directory, &new_directory).with_context(|| {
            format!(
                "could not rename profile directory `{}`",
                old_directory.display()
            )
        })?;
        let old_label =
            std::mem::replace(&mut self.store.profiles[index].label, new_label.to_string());
        self.store.profiles[index].config_dir = new_directory;
        if self.store.active.as_deref() == Some(&old_label) {
            self.store.active = Some(new_label.to_string());
        }
        self.save()?;
        println!("Renamed `{old_label}` to `{new_label}`.");
        Ok(())
    }

    fn reauth(&mut self, label: &str) -> Result<()> {
        let profile = self.profile(label).cloned().context("unknown profile")?;
        ensure_claude_available()?;
        println!(
            "Claude Code will open now. Complete its normal sign-in flow, then exit Claude Code."
        );
        let status = self.start_claude_for(&profile.label)?;
        if !status.success() {
            bail!("Claude Code exited unsuccessfully; profile was not changed");
        }
        let config = fs::read_to_string(self.profile_config_path(&profile.label))
            .context("no new Claude Code account state found")?;
        validate_profile_config(&config)?;
        let index = self.index_of(&profile.label).expect("profile exists");
        self.store.profiles[index].email = extract_account_email(&config);
        self.save()?;
        println!("Reauthenticated `{}`.", profile.label);
        Ok(())
    }

    fn remove(&mut self, label: &str, force: bool) -> Result<()> {
        let index = self.index_of(label).context("unknown profile")?;
        if self.store.active.as_deref() == Some(&self.store.profiles[index].label) {
            bail!("cannot remove the active profile; switch to another profile first");
        }
        let profile = self.store.profiles[index].clone();
        if !force
            && !confirm(&format!(
                "Remove `{}` ({})?",
                profile.label,
                profile.email.as_deref().unwrap_or("email unavailable")
            ))?
        {
            println!("Not removed.");
            return Ok(());
        }
        fs::remove_dir_all(&profile.config_dir).with_context(|| {
            format!("could not remove profile directory for `{}`", profile.label)
        })?;
        self.store.profiles.remove(index);
        self.save()?;
        println!("Removed `{}`.", profile.label);
        Ok(())
    }

    fn profile(&self, label: &str) -> Option<&Profile> {
        self.index_of(label)
            .map(|index| &self.store.profiles[index])
    }
    fn index_of(&self, label: &str) -> Option<usize> {
        self.store
            .profiles
            .iter()
            .position(|p| p.label.eq_ignore_ascii_case(label))
    }
    fn require_new_label(&self, label: &str) -> Result<()> {
        validate_label(label)?;
        if self.index_of(label).is_some() {
            bail!("a profile named `{label}` already exists");
        }
        Ok(())
    }
    fn save(&self) -> Result<()> {
        write_atomically(
            &self.metadata_path,
            &serde_json::to_string_pretty(&self.store)?,
        )
    }
    fn legacy_config_path(&self) -> PathBuf {
        self.claude_root.join(".claude.json")
    }
    fn profile_dir(&self, label: &str) -> PathBuf {
        self.profile(label)
            .map(|profile| profile.config_dir.clone())
            .unwrap_or_else(|| self.claude_root.join(label))
    }
    fn profile_config_path(&self, label: &str) -> PathBuf {
        self.profile_dir(label).join(".claude.json")
    }
    fn start_claude_for(&self, label: &str) -> Result<std::process::ExitStatus> {
        fs::create_dir_all(self.profile_dir(label))?;
        Command::new("claude")
            .env("CLAUDE_CONFIG_DIR", self.profile_dir(label))
            .status()
            .context("could not start Claude Code")
    }
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}
fn read_store(path: &Path) -> Result<Store> {
    if !path.exists() {
        return Ok(Store::default());
    }
    serde_json::from_str(&fs::read_to_string(path)?).context("profile metadata is invalid")
}
fn write_atomically(path: &Path, contents: &str) -> Result<()> {
    let parent = path.parent().context("path has no parent")?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.tmp",
        path.file_name().unwrap().to_string_lossy()
    ));
    fs::write(&temp, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(temp, path)?;
    Ok(())
}
fn validate_label(label: &str) -> Result<()> {
    if label.trim().is_empty()
        || label.trim() != label
        || label.len() > 64
        || label == "."
        || label == ".."
        || label.contains(['/', '\\', '\0'])
    {
        bail!(
            "profile labels must be 1-64 characters with no surrounding whitespace or path separators"
        );
    }
    Ok(())
}
fn shell_quote(path: &Path) -> String {
    format!(
        "'{}'",
        path.display().to_string().replace('\'', "'\\\"'\\\"'")
    )
}
fn validate_profile_config(config: &str) -> Result<()> {
    let value: Value = serde_json::from_str(config).context("account state is not valid JSON")?;
    if !value.get("oauthAccount").is_some_and(Value::is_object) {
        bail!("no signed-in Claude Code account was found");
    }
    Ok(())
}
fn extract_account_email(config: &str) -> Option<String> {
    serde_json::from_str::<Value>(config)
        .ok()
        .and_then(|value| {
            value
                .pointer("/oauthAccount/emailAddress")?
                .as_str()
                .map(str::to_owned)
        })
}
fn discover_profiles(claude_root: &Path, known_labels: &[String]) -> Result<Vec<Profile>> {
    if !claude_root.is_dir() {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    for entry in fs::read_dir(claude_root)? {
        let entry = entry?;
        let config_dir = entry.path();
        let Ok(label) = entry.file_name().into_string() else {
            continue;
        };
        if !config_dir.is_dir()
            || known_labels
                .iter()
                .any(|known| known == &label.to_ascii_lowercase())
        {
            continue;
        }
        let config_path = config_dir.join(".claude.json");
        let Ok(config) = fs::read_to_string(&config_path) else {
            continue;
        };
        if validate_profile_config(&config).is_ok() {
            profiles.push(Profile {
                label,
                email: extract_account_email(&config),
                config_dir,
            });
        }
    }
    profiles.sort_by_key(|profile| profile.label.to_ascii_lowercase());
    Ok(profiles)
}
fn ensure_claude_available() -> Result<()> {
    let status = Command::new("claude")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("could not execute Claude Code")?;
    if !status.success() {
        bail!("Claude Code was not found. Install it and ensure `claude` is on PATH first.");
    }
    Ok(())
}
fn claude_is_running() -> bool {
    Command::new("pgrep")
        .args(["-x", "claude"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_are_validated() {
        assert!(validate_label("work").is_ok());
        assert!(validate_label(" Work").is_err());
        assert!(validate_label("client/a").is_err());
        assert!(validate_label(".").is_err());
    }
    #[test]
    fn extracts_the_oauth_account_email() {
        assert_eq!(
            extract_account_email(r#"{"oauthAccount": {"emailAddress": "me@example.com"}}"#)
                .as_deref(),
            Some("me@example.com")
        );
    }
    #[test]
    fn profile_config_requires_a_signed_in_account() {
        assert!(validate_profile_config("{}").is_err());
        assert!(validate_profile_config(r#"{"oauthAccount": {}}"#).is_ok());
    }
    #[test]
    fn metadata_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("profiles.json");
        let store = Store {
            active: Some("work".to_string()),
            profiles: vec![Profile {
                label: "work".to_string(),
                email: Some("me@example.com".to_string()),
                config_dir: PathBuf::from("/Users/me/.claude/work"),
            }],
        };
        write_atomically(&path, &serde_json::to_string(&store).unwrap()).unwrap();
        assert_eq!(read_store(&path).unwrap().active.as_deref(), Some("work"));
    }
    #[test]
    fn shell_paths_are_quoted() {
        assert_eq!(
            shell_quote(Path::new("/Users/me/Claude's")),
            "'/Users/me/Claude'\\\"'\\\"'s'"
        );
    }
    #[test]
    fn legacy_metadata_defaults_the_profile_path_for_migration() {
        let profile: Profile = serde_json::from_str(r#"{"label":"work","email":null}"#).unwrap();
        assert!(profile.config_dir.as_os_str().is_empty());
    }
    #[test]
    fn discovers_unregistered_signed_in_profile_directories() {
        let directory = tempfile::tempdir().unwrap();
        let work = directory.path().join("work");
        fs::create_dir_all(&work).unwrap();
        fs::write(
            work.join(".claude.json"),
            r#"{"oauthAccount":{"emailAddress":"work@example.com"}}"#,
        )
        .unwrap();

        let profiles = discover_profiles(directory.path(), &["personal".to_string()]).unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].label, "work");
        assert_eq!(profiles[0].config_dir, work);
    }
}
