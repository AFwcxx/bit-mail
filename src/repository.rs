use std::{
    env, fs,
    fs::OpenOptions,
    io::{self, IsTerminal, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Result;

const REPOSITORY_SCHEMA_VERSION: u32 = 2;
const FILE_SCHEMA_VERSION: u32 = 1;
#[cfg(test)]
const MANAGED_DIRS: [&str; 4] = ["data", "knowledge", "skills", ".bit-mail"];
const MANAGED_PATHS: [&str; 5] = ["data", "knowledge", "skills", "AGENTS.md", ".bit-mail"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepositoryMetadata {
    pub schema_version: u32,
    pub id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_assets_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub pull: PullConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub oauth_clients: Vec<OAuthClientProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OAuthClientProfile {
    pub id: Uuid,
    pub alias: String,
    pub provider: String,
    pub client_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PullConfig {
    pub default_limit: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: FILE_SCHEMA_VERSION,
            pull: PullConfig { default_limit: 500 },
            oauth_clients: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Repository {
    root: PathBuf,
    metadata: RepositoryMetadata,
}

#[derive(Debug, Clone, Copy)]
pub enum GitIgnorePolicy {
    Prompt,
    Never,
}

impl Repository {
    pub fn initialize(root: &Path, policy: GitIgnorePolicy) -> Result<Self> {
        let root = fs::canonicalize(root)?;
        for name in MANAGED_PATHS {
            if root.join(name).exists() {
                return Err(message(format!(
                    "managed path already exists: {}",
                    root.join(name).display()
                )));
            }
        }

        let id = Uuid::new_v4();
        let staging = root.join(format!(".bit-mail-init-{id}"));
        create_private_dir(&staging)?;
        let result = (|| -> Result<()> {
            create_private_dir(&staging.join(".bit-mail"))?;
            create_private_dir(&staging.join(".bit-mail/accounts"))?;
            create_private_dir(&staging.join(".bit-mail/locks"))?;
            create_private_dir(&staging.join(".bit-mail/locks/accounts"))?;
            create_private_dir(&staging.join("data"))?;
            create_private_dir(&staging.join("knowledge"))?;
            create_private_dir(&staging.join("knowledge/global"))?;
            create_private_dir(&staging.join("knowledge/accounts"))?;
            create_private_dir(&staging.join("skills"))?;
            for (path, bytes) in crate::runtime_assets::ASSETS {
                let path = staging.join(path);
                if let Some(parent) = path.parent() {
                    create_private_dir(parent)?;
                }
                write_private(&path, bytes)?;
            }

            write_toml(
                &staging.join(".bit-mail/repository.toml"),
                &RepositoryMetadata {
                    schema_version: REPOSITORY_SCHEMA_VERSION,
                    id,
                    runtime_assets_version: Some(crate::runtime_assets::VERSION.to_owned()),
                },
            )?;
            write_toml(&staging.join(".bit-mail/config.toml"), &Config::default())?;
            write_private(
                &staging.join(".bit-mail/integrity-migration"),
                b"initial integrity build in progress\n",
            )?;

            publish_staged(&root, &staging)
        })();
        if let Err(error) = fs::remove_dir_all(&staging)
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!(
                "warning: failed to remove initialization staging directory {}: {error}",
                staging.display()
            );
        }
        result?;

        let repository = Self::open(root)?;
        crate::integrity::rebuild_full(&repository)?;
        fs::remove_file(repository.root.join(".bit-mail/integrity-migration"))?;
        if let Err(error) = protect_git_paths(repository.root(), policy) {
            eprintln!("warning: repository initialized, but Git ignore protection failed: {error}");
        }
        Ok(repository)
    }

    pub fn discover_from(start: &Path) -> Result<Self> {
        let start = fs::canonicalize(start)?;
        let start = if start.is_file() {
            start
                .parent()
                .ok_or_else(|| message("path has no parent"))?
        } else {
            &start
        };
        for root in start.ancestors() {
            if root.join(".bit-mail").is_dir() {
                let mut repository = Self::open(root.to_path_buf())?;
                repository.recover_runtime_asset_updates()?;
                repository.synchronize_runtime_assets()?;
                return Ok(repository);
            }
        }
        Err(message(
            "not inside a bit-mail repository; run `bit-mail init`",
        ))
    }

    pub fn discover_current() -> Result<Self> {
        Self::discover_from(&env::current_dir()?)
    }

    pub fn discover_current_for_diagnostics() -> Result<Self> {
        let start = fs::canonicalize(env::current_dir()?)?;
        for root in start.ancestors() {
            if root.join(".bit-mail").is_dir() {
                let mut repository = Self::open(root.to_path_buf())?;
                repository.recover_runtime_asset_updates()?;
                return Ok(repository);
            }
        }
        Err(message(
            "not inside a bit-mail repository; run `bit-mail init`",
        ))
    }

    pub fn open(root: PathBuf) -> Result<Self> {
        let metadata: RepositoryMetadata = read_toml(&root.join(".bit-mail/repository.toml"))?;
        require_repository_version(metadata.schema_version)?;
        let config: Config = read_toml(&root.join(".bit-mail/config.toml"))?;
        require_version(config.schema_version, "config")?;
        Ok(Self { root, metadata })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn id(&self) -> Uuid {
        self.metadata.id
    }

    pub fn runtime_assets_version(&self) -> Option<&str> {
        self.metadata.runtime_assets_version.as_deref()
    }

    pub(crate) fn require_current_runtime_assets(&self) -> Result<()> {
        if self.runtime_assets_version() == Some(crate::runtime_assets::VERSION) {
            return Ok(());
        }
        Err(message(format!(
            "runtime assets version does not match binary {}; reopen the repository to synchronize it",
            crate::runtime_assets::VERSION
        )))
    }

    fn synchronize_runtime_assets(&mut self) -> Result<()> {
        if self.metadata.schema_version != REPOSITORY_SCHEMA_VERSION
            || self.runtime_assets_version() == Some(crate::runtime_assets::VERSION)
        {
            return Ok(());
        }

        let _lifecycle = self.account_lifecycle_lock()?;
        let current = Self::open(self.root.clone())?;
        if current.runtime_assets_version() == Some(crate::runtime_assets::VERSION) {
            *self = current;
            return Ok(());
        }
        crate::integrity::prepare_repository(&current)?;
        if current.runtime_assets_version().is_none()
            && (current.root.join("AGENTS.md").exists()
                || directory_has_files(&current.root.join("skills"))?)
        {
            return Err(message(
                "legacy runtime asset collision; move the existing AGENTS.md/skills content before retrying",
            ));
        }

        // ponytail: returned errors roll back; M010 adds recovery after abrupt interruption.
        let transaction = current
            .root
            .join(".bit-mail")
            .join(format!("runtime-assets-update-{}", Uuid::new_v4()));
        let staged = transaction.join("new");
        let backup = transaction.join("old");
        create_private_dir(&staged.join("skills"))?;
        create_private_dir(&backup)?;
        fs::copy(
            current.root.join(".bit-mail/repository.toml"),
            backup.join("repository.toml"),
        )?;
        fs::copy(
            current.root.join(".bit-mail/integrity/repository.json"),
            backup.join("integrity-manifest.json"),
        )?;
        set_mode(&backup.join("repository.toml"), 0o600)?;
        set_mode(&backup.join("integrity-manifest.json"), 0o600)?;
        let staged_result = (|| -> Result<()> {
            for (path, bytes) in crate::runtime_assets::ASSETS {
                if *path != "AGENTS.md" && !path.starts_with("skills/") {
                    return Err(message(format!("unsupported runtime asset path: {path}")));
                }
                let target = staged.join(path);
                create_private_dir(target.parent().expect("runtime asset parent"))?;
                write_private(&target, bytes)?;
            }
            Ok(())
        })();
        if let Err(error) = staged_result {
            let _ = fs::remove_dir_all(&transaction);
            return Err(error);
        }

        let old_metadata = current.metadata.clone();
        let mut moved_skills = false;
        let mut moved_agents = false;
        let mut published_skills = false;
        let mut published_agents = false;
        let publication = (|| -> Result<()> {
            if current.root.join("skills").exists() {
                fs::rename(current.root.join("skills"), backup.join("skills"))?;
                moved_skills = true;
            }
            if current.root.join("AGENTS.md").exists() {
                fs::rename(current.root.join("AGENTS.md"), backup.join("AGENTS.md"))?;
                moved_agents = true;
            }
            fs::rename(staged.join("skills"), current.root.join("skills"))?;
            published_skills = true;
            fs::rename(staged.join("AGENTS.md"), current.root.join("AGENTS.md"))?;
            published_agents = true;

            let mut metadata = old_metadata.clone();
            metadata.runtime_assets_version = Some(crate::runtime_assets::VERSION.to_owned());
            write_toml_atomic(&current.root.join(".bit-mail/repository.toml"), &metadata)?;
            crate::integrity::commit_repository(&current)
        })();

        if let Err(error) = publication {
            let mut rollback = Vec::new();
            if published_agents
                && let Err(failure) = fs::remove_file(current.root.join("AGENTS.md"))
            {
                rollback.push(failure.to_string());
            }
            if published_skills
                && let Err(failure) = fs::remove_dir_all(current.root.join("skills"))
            {
                rollback.push(failure.to_string());
            }
            if moved_agents
                && let Err(failure) =
                    fs::rename(backup.join("AGENTS.md"), current.root.join("AGENTS.md"))
            {
                rollback.push(failure.to_string());
            }
            if moved_skills
                && let Err(failure) = fs::rename(backup.join("skills"), current.root.join("skills"))
            {
                rollback.push(failure.to_string());
            }
            if let Err(failure) = write_toml_atomic(
                &current.root.join(".bit-mail/repository.toml"),
                &old_metadata,
            ) {
                rollback.push(failure.to_string());
            }
            if let Err(failure) = crate::integrity::commit_repository(&current) {
                rollback.push(failure.to_string());
            }
            let _ = fs::remove_dir_all(&transaction);
            if rollback.is_empty() {
                return Err(error);
            }
            return Err(message(format!(
                "{error}; runtime asset rollback also failed: {}",
                rollback.join(", ")
            )));
        }

        if let Err(error) = fs::remove_dir_all(&transaction) {
            eprintln!(
                "warning: runtime assets synchronized, but cleanup failed for {}: {error}",
                transaction.display()
            );
        }
        *self = Self::open(self.root.clone())?;
        Ok(())
    }

    fn recover_runtime_asset_updates(&mut self) -> Result<()> {
        let internal = self.root.join(".bit-mail");
        let mut transactions = fs::read_dir(&internal)?
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                path.file_name()?
                    .to_string_lossy()
                    .starts_with("runtime-assets-update-")
                    .then_some(path)
            })
            .collect::<Vec<_>>();
        transactions.sort();
        if transactions.is_empty() {
            return Ok(());
        }
        let _lifecycle = self.account_lifecycle_lock()?;
        for transaction in transactions {
            if crate::integrity::validate_repository(self).is_ok_and(|v| v.is_empty()) {
                fs::remove_dir_all(transaction)?;
                continue;
            }
            let backup = transaction.join("old");
            let old_metadata: RepositoryMetadata = read_toml(&backup.join("repository.toml"))?;
            for name in ["skills", "AGENTS.md"] {
                let saved = backup.join(name);
                let live = self.root.join(name);
                if !saved.exists() && old_metadata.runtime_assets_version.is_some() {
                    continue;
                }
                if live.is_dir() {
                    fs::remove_dir_all(&live)?;
                } else if live.exists() {
                    fs::remove_file(&live)?;
                }
                if saved.exists() {
                    fs::rename(saved, live)?;
                }
            }
            for (saved, live) in [
                (
                    backup.join("repository.toml"),
                    internal.join("repository.toml"),
                ),
                (
                    backup.join("integrity-manifest.json"),
                    internal.join("integrity/repository.json"),
                ),
            ] {
                if !saved.is_file() {
                    return Err(message(
                        "interrupted runtime asset update has no integrity-valid backup; restore the runtime repository from a trusted filesystem backup",
                    ));
                }
                if live.exists() {
                    fs::remove_file(&live)?;
                }
                fs::rename(saved, live)?;
            }
            *self = Self::open(self.root.clone())?;
            if !crate::integrity::validate_repository(self)?.is_empty() {
                return Err(message(
                    "interrupted runtime asset backup failed integrity validation; restore the runtime repository from a trusted filesystem backup",
                ));
            }
            fs::remove_dir_all(transaction)?;
        }
        Ok(())
    }

    pub(crate) fn require_integrity_ready(&self) -> Result<()> {
        if self.metadata.schema_version != REPOSITORY_SCHEMA_VERSION {
            return Err(message(
                "repository integrity is not initialized; run `bit-mail migrate-integrity`",
            ));
        }
        if self.root.join(".bit-mail/integrity-migration").exists() {
            return Err(message(
                "repository integrity migration is incomplete; rerun `bit-mail migrate-integrity`",
            ));
        }
        Ok(())
    }

    pub fn migrate_integrity(&self) -> Result<bool> {
        let marker = self.root.join(".bit-mail/integrity-migration");
        if self.metadata.schema_version == REPOSITORY_SCHEMA_VERSION && !marker.exists() {
            return Ok(false);
        }
        let _lifecycle = self.account_lifecycle_lock()?;
        let mut accounts = self.accounts()?;
        accounts.sort_by_key(|account| account.id);
        let _account_locks = accounts
            .iter()
            .map(|account| self.account_lock(account.id))
            .collect::<Result<Vec<_>>>()?;
        let _knowledge = self.knowledge_lock()?;
        if !marker.exists() {
            write_private(&marker, b"integrity schema v1 migration in progress\n")?;
        }
        write_toml_atomic(
            &self.root.join(".bit-mail/repository.toml"),
            &RepositoryMetadata {
                schema_version: REPOSITORY_SCHEMA_VERSION,
                id: self.metadata.id,
                runtime_assets_version: self.metadata.runtime_assets_version.clone(),
            },
        )?;
        let current = Self::open(self.root.clone())?;
        crate::integrity::rebuild_full(&current)?;
        fs::remove_file(marker)?;
        Ok(true)
    }

    pub fn config(&self) -> Result<Config> {
        let config: Config = read_toml(&self.root.join(".bit-mail/config.toml"))?;
        require_version(config.schema_version, "config")?;
        Ok(config)
    }

    pub fn set_config(&self, key: &str, value: &str) -> Result<Config> {
        let _lifecycle_lock = self.account_lifecycle_lock()?;
        crate::integrity::prepare_repository(self)?;
        let mut config = self.config()?;
        match key {
            "pull.default-limit" => {
                let value: u32 = value.parse()?;
                if value == 0 {
                    return Err(message("pull.default-limit must be greater than zero"));
                }
                config.pull.default_limit = value;
            }
            _ => return Err(message(format!("unsupported config key: {key}"))),
        }
        write_toml_atomic(&self.root.join(".bit-mail/config.toml"), &config)?;
        crate::integrity::commit_repository(self)?;
        Ok(config)
    }

    pub fn add_oauth_profile(&self, profile: OAuthClientProfile) -> Result<()> {
        validate_alias(&profile.alias)?;
        if profile.provider != "google" || profile.client_id.trim().is_empty() {
            return Err(message("invalid Google OAuth client profile"));
        }
        let _lifecycle_lock = self.account_lifecycle_lock()?;
        crate::integrity::prepare_repository(self)?;
        let mut config = self.config()?;
        if config
            .oauth_clients
            .iter()
            .any(|item| item.alias == profile.alias)
        {
            return Err(message(format!(
                "OAuth client profile already exists: {}",
                profile.alias
            )));
        }
        config.oauth_clients.push(profile);
        config.oauth_clients.sort_by(|a, b| a.alias.cmp(&b.alias));
        write_toml_atomic(&self.root.join(".bit-mail/config.toml"), &config)?;
        crate::integrity::commit_repository(self)
    }

    pub fn config_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(&self.config()?)?)
    }

    pub fn config_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(&self.config()?)?)
    }
}

fn publish_staged(root: &Path, staging: &Path) -> Result<()> {
    let mut moved: Vec<PathBuf> = Vec::new();
    for name in MANAGED_PATHS {
        let target = root.join(name);
        let publish = if target.exists() {
            Err(message(format!(
                "managed path appeared during initialization: {}",
                target.display()
            )))
        } else {
            fs::rename(staging.join(name), &target).map_err(Into::into)
        };
        if let Err(error) = publish {
            let mut rollback_failures = Vec::new();
            for path in moved.into_iter().rev() {
                let rollback = if path.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
                if let Err(rollback_error) = rollback {
                    rollback_failures.push(format!("{}: {rollback_error}", path.display()));
                }
            }
            if rollback_failures.is_empty() {
                return Err(error);
            }
            return Err(message(format!(
                "{error}; initialization rollback also failed: {}",
                rollback_failures.join(", ")
            )));
        }
        moved.push(target);
    }
    Ok(())
}

fn directory_has_files(path: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            || entry.file_type()?.is_symlink()
            || entry.file_type()?.is_dir() && directory_has_files(&entry.path())?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn require_repository_version(version: u32) -> Result<()> {
    if matches!(version, 1 | REPOSITORY_SCHEMA_VERSION) {
        Ok(())
    } else {
        Err(message(format!(
            "unsupported repository schema version {version}; supported versions are 1 and {REPOSITORY_SCHEMA_VERSION}"
        )))
    }
}

fn require_version(version: u32, kind: &str) -> Result<()> {
    if version == FILE_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(message(format!(
            "unsupported {kind} schema version {version}; supported version is {FILE_SCHEMA_VERSION}"
        )))
    }
}

fn protect_git_paths(root: &Path, policy: GitIgnorePolicy) -> Result<()> {
    let Some(git_root) = root
        .ancestors()
        .find(|path| is_git_marker(&path.join(".git")))
        .map(Path::to_path_buf)
    else {
        return Ok(());
    };
    let relative = root.strip_prefix(&git_root)?;
    let prefix = match relative.components().next() {
        None => String::new(),
        Some(Component::Normal(_)) => format!("{}/", relative.display()),
        _ => {
            return Err(message(
                "runtime repository is not below the detected Git root",
            ));
        }
    };
    let private_paths = [".bit-mail", "data", "knowledge"].map(|path| relative.join(path));
    let rules = [".bit-mail/", "data/", "knowledge/"].map(|path| format!("/{prefix}{path}"));
    let ignore_path = git_root.join(".gitignore");
    let existing = fs::read_to_string(&ignore_path).unwrap_or_default();
    let missing: Vec<_> = rules
        .iter()
        .zip(private_paths.iter())
        .filter(|(rule, path)| {
            !git_ignores(&git_root, path)
                && !existing.lines().any(|line| line.trim() == rule.as_str())
        })
        .map(|(rule, _)| rule)
        .collect();
    for path in &private_paths {
        if git_tracks(&git_root, path) {
            eprintln!(
                "warning: private runtime path is already Git-tracked: {}",
                git_root.join(path).display()
            );
        }
    }
    if missing.is_empty() {
        return Ok(());
    }

    let mut append = false;
    if matches!(policy, GitIgnorePolicy::Prompt) && io::stdin().is_terminal() {
        eprint!(
            "Git repository detected. Add private bit-mail paths to {}? [y/N] ",
            ignore_path.display()
        );
        io::stderr().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        append = matches!(answer.trim(), "y" | "Y" | "yes" | "YES");
    }
    if append {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&ignore_path)?;
        if !existing.is_empty() && !existing.ends_with('\n') {
            writeln!(file)?;
        }
        for rule in missing {
            writeln!(file, "{rule}")?;
        }
    } else {
        eprintln!(
            "warning: add these private paths to {}:",
            ignore_path.display()
        );
        for rule in missing {
            eprintln!("  {rule}");
        }
    }
    Ok(())
}

fn is_git_marker(path: &Path) -> bool {
    path.is_file() || path.join("HEAD").is_file()
}

fn git_ignores(git_root: &Path, path: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(git_root)
        .args(["check-ignore", "--quiet", "--"])
        .arg(path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn git_tracks(git_root: &Path, path: &Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(git_root)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(path)
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success() && !output.stdout.is_empty())
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    Ok(toml::from_str(&fs::read_to_string(path)?)?)
}

fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_private(path, toml::to_string_pretty(value)?.as_bytes())
}

fn write_toml_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    write_toml(&temporary, value)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    set_mode(path, 0o700)?;
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    set_mode(path, 0o600)?;
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

fn message(text: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    io::Error::other(text.into()).into()
}

#[derive(Debug)]
pub struct MutationLock {
    path: PathBuf,
}

impl MutationLock {
    pub fn acquire(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            create_private_dir(parent)?;
        }
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let holder = fs::read_to_string(&path)
                    .unwrap_or_else(|_| "unreadable holder metadata".into());
                return Err(message(format!(
                    "lock is held at {} ({holder}); if the process no longer exists, diagnose before removing this possibly stale lock",
                    path.display()
                )));
            }
            Err(error) => return Err(error.into()),
        };
        let setup = (|| -> Result<()> {
            writeln!(
                file,
                "pid={} acquired_unix={}",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs()
            )?;
            set_mode(&path, 0o600)
        })();
        if let Err(error) = setup {
            drop(file);
            if let Err(cleanup_error) = fs::remove_file(&path) {
                return Err(message(format!(
                    "{error}; failed to clean up incomplete lock {}: {cleanup_error}",
                    path.display()
                )));
            }
            return Err(error);
        }
        Ok(Self { path })
    }
}

impl Drop for MutationLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AccountConfig {
    pub schema_version: u32,
    pub id: Uuid,
    pub alias: String,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_identity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_profile: Option<String>,
}

#[derive(Debug)]
pub struct NewAccount<'a> {
    pub alias: &'a str,
    pub provider: &'a str,
    pub provider_identity: Option<&'a str>,
    pub credential_profile: Option<&'a str>,
}

#[derive(Debug, Clone, Copy)]
pub struct RemoveOptions {
    pub discard_local_data: bool,
    pub keep_credentials: bool,
    pub revoke_credentials: bool,
}

pub trait CredentialRevoker {
    fn revoke(&self, repository_id: Uuid, account: &AccountConfig) -> Result<()>;
}

pub struct UnavailableCredentialRevoker;

impl CredentialRevoker for UnavailableCredentialRevoker {
    fn revoke(&self, _repository_id: Uuid, _account: &AccountConfig) -> Result<()> {
        Err(message(
            "credential revocation requires the secure credential backend implemented by M003",
        ))
    }
}

impl Repository {
    pub fn accounts(&self) -> Result<Vec<AccountConfig>> {
        let mut accounts = Vec::new();
        for entry in fs::read_dir(self.root.join(".bit-mail/accounts"))? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let path = entry.path().join("account.toml");
            if !path.exists() {
                return Err(message(format!(
                    "missing account config: {}",
                    path.display()
                )));
            }
            let account: AccountConfig = read_toml(&path)?;
            require_version(account.schema_version, "account")?;
            if entry.file_name().to_string_lossy() != account.id.to_string() {
                return Err(message(format!(
                    "account UUID does not match its directory: {}",
                    path.display()
                )));
            }
            validate_alias(&account.alias)?;
            accounts.push(account);
        }
        accounts.sort_by(|left, right| left.alias.cmp(&right.alias));
        Ok(accounts)
    }

    pub fn create_account(&self, new: NewAccount<'_>) -> Result<AccountConfig> {
        self.create_account_with_id(Uuid::new_v4(), new)
    }

    pub(crate) fn create_account_with_id(
        &self,
        id: Uuid,
        new: NewAccount<'_>,
    ) -> Result<AccountConfig> {
        self.require_integrity_ready()?;
        validate_alias(new.alias)?;
        let _lifecycle_lock = self.account_lifecycle_lock()?;
        let accounts = self.accounts()?;
        if accounts.iter().any(|account| account.alias == new.alias) {
            return Err(message(format!(
                "account alias already exists: {}",
                new.alias
            )));
        }
        if let Some(identity) = new.provider_identity
            && accounts.iter().any(|account| {
                account.provider == new.provider
                    && account
                        .provider_identity
                        .as_ref()
                        .is_some_and(|existing| existing.eq_ignore_ascii_case(identity))
            })
        {
            return Err(message(format!(
                "provider mailbox identity is already configured: {identity}"
            )));
        }

        let account = AccountConfig {
            schema_version: FILE_SCHEMA_VERSION,
            id,
            alias: new.alias.to_owned(),
            provider: new.provider.to_owned(),
            provider_identity: new.provider_identity.map(str::to_owned),
            credential_profile: new.credential_profile.map(str::to_owned),
        };
        let account_dir = self.account_dir(account.id);
        let data_dir = self.data_dir(account.id);
        let staging = self
            .root
            .join(".bit-mail")
            .join(format!("account-init-{}", account.id));
        if account_dir.exists() || data_dir.exists() || staging.exists() {
            return Err(message(
                "generated account UUID collides with existing state",
            ));
        }
        create_private_dir(&staging)?;
        let result = (|| -> Result<()> {
            create_private_dir(&data_dir)?;
            write_toml(&staging.join("account.toml"), &account)?;
            fs::rename(&staging, &account_dir)?;
            Ok(())
        })();
        if let Err(error) = result {
            let mut rollback_failures = Vec::new();
            for path in [&staging, &account_dir, &data_dir] {
                if let Err(rollback_error) = fs::remove_dir_all(path)
                    && rollback_error.kind() != io::ErrorKind::NotFound
                {
                    rollback_failures.push(format!("{}: {rollback_error}", path.display()));
                }
            }
            if rollback_failures.is_empty() {
                return Err(error);
            }
            return Err(message(format!(
                "{error}; account creation rollback also failed: {}",
                rollback_failures.join(", ")
            )));
        }
        crate::integrity::reset_account(self, account.id)?;
        Ok(account)
    }

    pub fn account_by_alias(&self, alias: &str) -> Result<AccountConfig> {
        self.accounts()?
            .into_iter()
            .find(|account| account.alias == alias)
            .ok_or_else(|| message(format!("unknown account alias: {alias}")))
    }

    pub fn rename_account(&self, old_alias: &str, new_alias: &str) -> Result<AccountConfig> {
        validate_alias(new_alias)?;
        let _lifecycle_lock = self.account_lifecycle_lock()?;
        let accounts = self.accounts()?;
        if accounts.iter().any(|account| account.alias == new_alias) {
            return Err(message(format!(
                "account alias already exists: {new_alias}"
            )));
        }
        let mut account = accounts
            .into_iter()
            .find(|account| account.alias == old_alias)
            .ok_or_else(|| message(format!("unknown account alias: {old_alias}")))?;
        let _lock = self.account_lock(account.id)?;
        crate::integrity::prepare_account(self, account.id)?;
        account.alias = new_alias.to_owned();
        write_toml_atomic(&self.account_dir(account.id).join("account.toml"), &account)?;
        crate::integrity::commit_account(self, account.id)?;
        Ok(account)
    }

    pub fn remove_account(
        &self,
        alias: &str,
        options: RemoveOptions,
        revoker: &dyn CredentialRevoker,
    ) -> Result<()> {
        let _lifecycle_lock = self.account_lifecycle_lock()?;
        let account = self.account_by_alias(alias)?;
        let _lock = self.account_lock(account.id)?;
        if options.discard_local_data {
            crate::integrity::bootstrap_account(self, account.id)?;
            crate::integrity::validate_preserved_account(self, account.id)?;
        } else {
            crate::integrity::prepare_account(self, account.id)?;
        }
        let account_dir = self.account_dir(account.id);
        let data_dir = self.data_dir(account.id);
        if has_meaningful_account_state(&account_dir, &data_dir)? && !options.discard_local_data {
            return Err(message(
                "account has local state; pass --discard-local-data to remove it",
            ));
        }
        if account.credential_profile.is_some() {
            match (options.keep_credentials, options.revoke_credentials) {
                (true, false) => {}
                (false, true) => revoker.revoke(self.id(), &account)?,
                _ => {
                    return Err(message(
                        "choose exactly one of --keep-credentials or --revoke-credentials",
                    ));
                }
            }
        }
        if data_dir.exists() {
            fs::remove_dir_all(data_dir)?;
        }
        let knowledge_dir = self
            .root
            .join("knowledge/accounts")
            .join(account.id.to_string());
        if knowledge_dir.is_dir() {
            crate::integrity::commit_orphan_knowledge(self, account.id)?;
        }
        fs::remove_dir_all(account_dir)?;
        if knowledge_dir.exists() {
            eprintln!(
                "warning: preserved account Knowledge at {}",
                knowledge_dir.display()
            );
        }
        Ok(())
    }

    pub fn resolve_account(
        &self,
        explicit: Option<&str>,
        cwd: &Path,
        environment_alias: Option<&str>,
    ) -> Result<AccountConfig> {
        if let Some(alias) = explicit {
            return self.account_by_alias(alias);
        }
        let cwd_account = self.account_from_path(cwd)?;
        let environment_account = environment_alias
            .map(|alias| self.account_by_alias(alias))
            .transpose()?;
        if let (Some(left), Some(right)) = (&cwd_account, &environment_account)
            && left.id != right.id
        {
            return Err(message(format!(
                "conflicting implicit accounts: current directory selects `{}`, BIT_MAIL_ACCOUNT selects `{}`",
                left.alias, right.alias
            )));
        }
        if let Some(account) = cwd_account.or(environment_account) {
            return Ok(account);
        }
        let accounts = self.accounts()?;
        match accounts.as_slice() {
            [account] => Ok(account.clone()),
            [] => Err(message(
                "no accounts are configured; run `bit-mail connect`",
            )),
            _ => Err(message(
                "multiple accounts are configured; pass --account <alias>",
            )),
        }
    }

    pub fn account_lock(&self, account_id: Uuid) -> Result<MutationLock> {
        let lock = MutationLock::acquire(
            self.root
                .join(".bit-mail/locks/accounts")
                .join(format!("{account_id}.lock")),
        )?;
        let account: AccountConfig = read_toml(&self.account_dir(account_id).join("account.toml"))?;
        if account.id != account_id {
            return Err(message("account UUID does not match its lock identity"));
        }
        Ok(lock)
    }

    pub fn knowledge_lock(&self) -> Result<MutationLock> {
        MutationLock::acquire(self.root.join(".bit-mail/locks/knowledge.lock"))
    }

    fn account_lifecycle_lock(&self) -> Result<MutationLock> {
        // ponytail: lifecycle/config writes are rare; split this lock only if contention is measurable.
        MutationLock::acquire(self.root.join(".bit-mail/locks/account-lifecycle.lock"))
    }

    pub fn data_dir(&self, account_id: Uuid) -> PathBuf {
        self.root.join("data").join(account_id.to_string())
    }

    fn account_dir(&self, account_id: Uuid) -> PathBuf {
        self.root
            .join(".bit-mail/accounts")
            .join(account_id.to_string())
    }

    fn account_from_path(&self, cwd: &Path) -> Result<Option<AccountConfig>> {
        let cwd = fs::canonicalize(cwd)?;
        let data = fs::canonicalize(self.root.join("data"))?;
        let Ok(relative) = cwd.strip_prefix(data) else {
            return Ok(None);
        };
        let Some(Component::Normal(id)) = relative.components().next() else {
            return Ok(None);
        };
        let Ok(id) = Uuid::parse_str(&id.to_string_lossy()) else {
            return Ok(None);
        };
        Ok(self
            .accounts()?
            .into_iter()
            .find(|account| account.id == id))
    }
}

pub(crate) fn validate_alias(alias: &str) -> Result<()> {
    let bytes = alias.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 32
        || !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit()
        || !bytes[bytes.len() - 1].is_ascii_lowercase() && !bytes[bytes.len() - 1].is_ascii_digit()
        || !bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
    {
        return Err(message(
            "account alias must be 1-32 lowercase ASCII letters/digits with internal '-' or '_'",
        ));
    }
    Ok(())
}

fn has_meaningful_account_state(account_dir: &Path, data_dir: &Path) -> Result<bool> {
    if data_dir.exists() && fs::read_dir(data_dir)?.next().is_some() {
        return Ok(true);
    }
    for entry in fs::read_dir(account_dir)? {
        let name = entry?.file_name();
        if name != "account.toml" && name != "integrity" {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use std::{fs, thread};

    use tempfile::TempDir;

    use super::*;

    fn repository() -> (TempDir, Repository) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never)
            .expect("repository initialization");
        (directory, repository)
    }

    fn add_account(repository: &Repository, alias: &str, identity: &str) -> AccountConfig {
        repository
            .create_account(NewAccount {
                alias,
                provider: "gmail",
                provider_identity: Some(identity),
                credential_profile: None,
            })
            .expect("account creation")
    }

    #[test]
    fn init_preflights_every_managed_path_before_mutating() {
        let directory = tempfile::tempdir().expect("temporary directory");
        fs::write(directory.path().join("notes.txt"), "keep").expect("ordinary file");
        fs::create_dir(directory.path().join("data")).expect("collision");

        let error = Repository::initialize(directory.path(), GitIgnorePolicy::Never)
            .expect_err("managed collision must fail");

        assert!(error.to_string().contains("managed path already exists"));
        assert!(!directory.path().join(".bit-mail").exists());
        assert!(!directory.path().join("knowledge").exists());
        assert_eq!(
            fs::read_to_string(directory.path().join("notes.txt")).expect("ordinary file retained"),
            "keep"
        );
    }

    #[test]
    fn init_installs_versioned_integrity_covered_runtime_assets() {
        let (_directory, repository) = repository();
        assert_eq!(
            repository.runtime_assets_version(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        for (path, bytes) in crate::runtime_assets::ASSETS {
            assert_eq!(fs::read(repository.root().join(path)).unwrap(), *bytes);
        }
        fs::write(repository.root().join("AGENTS.md"), "tampered").unwrap();
        assert!(
            crate::integrity::validate_full(&repository)
                .unwrap()
                .mismatches
                .iter()
                .any(|mismatch| mismatch.path.ends_with("AGENTS.md"))
        );
    }

    #[test]
    fn discovery_safely_synchronizes_legacy_and_older_runtime_assets() {
        let (_directory, repository) = repository();
        fs::remove_file(repository.root().join("AGENTS.md")).unwrap();
        fs::remove_dir_all(repository.root().join("skills")).unwrap();
        fs::create_dir(repository.root().join("skills")).unwrap();
        let mut metadata = repository.metadata.clone();
        metadata.runtime_assets_version = None;
        write_toml_atomic(
            &repository.root().join(".bit-mail/repository.toml"),
            &metadata,
        )
        .unwrap();
        crate::integrity::commit_repository(&repository).unwrap();

        let synchronized = Repository::discover_from(repository.root()).unwrap();
        assert_eq!(
            synchronized.runtime_assets_version(),
            Some(crate::runtime_assets::VERSION)
        );
        for (path, bytes) in crate::runtime_assets::ASSETS {
            assert_eq!(fs::read(synchronized.root().join(path)).unwrap(), *bytes);
        }

        fs::write(synchronized.root().join("skills/obsolete.md"), "old").unwrap();
        let mut metadata = synchronized.metadata.clone();
        metadata.runtime_assets_version = Some("older".into());
        write_toml_atomic(
            &synchronized.root().join(".bit-mail/repository.toml"),
            &metadata,
        )
        .unwrap();
        crate::integrity::commit_repository(&synchronized).unwrap();
        let upgraded = Repository::discover_from(synchronized.root()).unwrap();
        assert!(!upgraded.root().join("skills/obsolete.md").exists());
        assert!(
            crate::integrity::validate_full(&upgraded)
                .unwrap()
                .mismatches
                .is_empty()
        );
    }

    #[test]
    fn legacy_runtime_collision_and_tampered_upgrade_fail_unchanged() {
        let (_directory, repository) = repository();
        fs::remove_dir_all(repository.root().join("skills")).unwrap();
        fs::create_dir(repository.root().join("skills")).unwrap();
        fs::write(repository.root().join("AGENTS.md"), "user-owned").unwrap();
        let mut metadata = repository.metadata.clone();
        metadata.runtime_assets_version = None;
        write_toml_atomic(
            &repository.root().join(".bit-mail/repository.toml"),
            &metadata,
        )
        .unwrap();
        crate::integrity::commit_repository(&repository).unwrap();
        assert!(Repository::discover_from(repository.root()).is_err());
        assert_eq!(
            fs::read_to_string(repository.root().join("AGENTS.md")).unwrap(),
            "user-owned"
        );

        metadata.runtime_assets_version = Some("older".into());
        write_toml_atomic(
            &repository.root().join(".bit-mail/repository.toml"),
            &metadata,
        )
        .unwrap();
        crate::integrity::commit_repository(&repository).unwrap();
        fs::write(repository.root().join("AGENTS.md"), "tampered").unwrap();
        assert!(Repository::discover_from(repository.root()).is_err());
        assert_eq!(
            fs::read_to_string(repository.root().join("AGENTS.md")).unwrap(),
            "tampered"
        );
    }

    #[test]
    fn init_rejects_an_existing_runtime_bootstrap_without_partial_state() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("AGENTS.md"), "user-owned").unwrap();
        assert!(
            Repository::initialize(directory.path(), GitIgnorePolicy::Never)
                .unwrap_err()
                .to_string()
                .contains("managed path already exists")
        );
        assert!(!directory.path().join(".bit-mail").exists());
        assert_eq!(
            fs::read_to_string(directory.path().join("AGENTS.md")).unwrap(),
            "user-owned"
        );
    }

    #[test]
    fn init_rolls_back_if_publication_fails_before_the_repository_marker() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let staging = directory.path().join("staging");
        for name in MANAGED_DIRS {
            fs::create_dir_all(staging.join(name)).expect("staged managed directory");
        }
        fs::write(directory.path().join("knowledge"), "collision")
            .expect("mid-publication collision");

        let error = publish_staged(directory.path(), &staging)
            .expect_err("partial publication must roll back");

        assert!(error.to_string().contains("managed path appeared"));
        assert!(!directory.path().join("data").exists());
        assert!(!directory.path().join(".bit-mail").exists());
        assert_eq!(
            fs::read_to_string(directory.path().join("knowledge")).expect("collision retained"),
            "collision"
        );
    }

    #[test]
    fn discovery_finds_the_nearest_repository_from_nested_content() {
        let (_directory, repository) = repository();
        let nested = repository.root().join("ordinary/nested");
        fs::create_dir_all(&nested).expect("nested directory");

        let discovered = Repository::discover_from(&nested).expect("discover repository");

        assert_eq!(discovered.id(), repository.id());
        assert_eq!(discovered.root(), repository.root());
    }

    #[test]
    fn config_accepts_only_the_supported_non_secret_key() {
        let (_directory, repository) = repository();

        assert_eq!(
            repository
                .set_config("pull.default-limit", "1000")
                .expect("supported update")
                .pull
                .default_limit,
            1000
        );
        assert!(repository.set_config("oauth.token", "secret").is_err());
        assert_eq!(
            repository
                .config()
                .expect("unchanged config")
                .pull
                .default_limit,
            1000
        );
    }

    #[test]
    fn newer_repository_schema_is_never_reinterpreted() {
        let (_directory, repository) = repository();
        fs::write(
            repository.root().join(".bit-mail/repository.toml"),
            format!("schema_version = 3\nid = \"{}\"\n", repository.id()),
        )
        .expect("replace metadata for compatibility test");

        let error = Repository::open(repository.root().to_path_buf())
            .expect_err("newer schema must be rejected");

        assert!(
            error
                .to_string()
                .contains("unsupported repository schema version 3")
        );
    }

    #[test]
    fn integrity_migration_is_explicit_and_missing_v2_manifests_fail_closed() {
        let (_directory, repository) = repository();
        fs::write(
            repository.root().join(".bit-mail/repository.toml"),
            format!("schema_version = 1\nid = \"{}\"\n", repository.id()),
        )
        .unwrap();
        let legacy = Repository::open(repository.root().to_path_buf()).unwrap();
        assert!(
            legacy
                .create_account(NewAccount {
                    alias: "blocked",
                    provider: "gmail",
                    provider_identity: None,
                    credential_profile: None,
                })
                .unwrap_err()
                .to_string()
                .contains("migrate-integrity")
        );
        assert!(legacy.migrate_integrity().unwrap());
        let current = Repository::open(repository.root().to_path_buf()).unwrap();
        let account = add_account(&current, "mail", "mail@example.com");
        fs::write(
            current.root().join(".bit-mail/integrity-migration"),
            "interrupted",
        )
        .unwrap();
        fs::remove_file(
            current
                .account_dir(account.id)
                .join("integrity/manifest.json"),
        )
        .unwrap();
        assert!(current.migrate_integrity().unwrap());
        assert!(
            crate::integrity::validate_account(&current, account.id)
                .unwrap()
                .is_empty()
        );
        fs::remove_file(
            current
                .account_dir(account.id)
                .join("integrity/manifest.json"),
        )
        .unwrap();
        assert!(current.rename_account("mail", "renamed").is_err());
    }

    #[test]
    fn account_alias_changes_without_moving_uuid_owned_data() {
        let (_directory, repository) = repository();
        let account = add_account(&repository, "personal", "person@example.com");
        let data_path = repository.data_dir(account.id);

        let renamed = repository
            .rename_account("personal", "private_mail")
            .expect("rename account");

        assert_eq!(renamed.id, account.id);
        assert_eq!(renamed.alias, "private_mail");
        assert!(data_path.exists());
        assert!(repository.account_by_alias("personal").is_err());
    }

    #[test]
    fn duplicate_alias_and_provider_identity_fail_closed() {
        let (_directory, repository) = repository();
        add_account(&repository, "personal", "person@example.com");

        assert!(
            repository
                .create_account(NewAccount {
                    alias: "personal",
                    provider: "gmail",
                    provider_identity: Some("other@example.com"),
                    credential_profile: None,
                })
                .is_err()
        );
        assert!(
            repository
                .create_account(NewAccount {
                    alias: "case-variant",
                    provider: "gmail",
                    provider_identity: Some("PERSON@EXAMPLE.COM"),
                    credential_profile: None,
                })
                .is_err()
        );
        assert!(
            repository
                .create_account(NewAccount {
                    alias: "other",
                    provider: "gmail",
                    provider_identity: Some("person@example.com"),
                    credential_profile: None,
                })
                .is_err()
        );
        assert!(
            repository
                .create_account(NewAccount {
                    alias: "Invalid Alias",
                    provider: "gmail",
                    provider_identity: None,
                    credential_profile: None,
                })
                .is_err()
        );
    }

    #[test]
    fn explicit_account_wins_and_implicit_conflicts_fail() {
        let (_directory, repository) = repository();
        let personal = add_account(&repository, "personal", "person@example.com");
        let work = add_account(&repository, "work", "work@example.com");
        let nested = repository.data_dir(personal.id).join("messages");
        fs::create_dir(&nested).expect("account content directory");

        assert_eq!(
            repository
                .resolve_account(Some("work"), &nested, Some("personal"))
                .expect("explicit selection")
                .id,
            work.id
        );
        assert!(
            repository
                .resolve_account(None, &nested, Some("work"))
                .is_err()
        );
        assert_eq!(
            repository
                .resolve_account(None, repository.root(), Some("work"))
                .expect("environment selection")
                .id,
            work.id
        );
    }

    #[test]
    fn removal_requires_discard_for_meaningful_local_state() {
        let (_directory, repository) = repository();
        let account = add_account(&repository, "personal", "person@example.com");
        fs::write(repository.data_dir(account.id).join("mail"), "private").expect("local state");

        assert!(
            repository
                .remove_account(
                    "personal",
                    RemoveOptions {
                        discard_local_data: false,
                        keep_credentials: false,
                        revoke_credentials: false,
                    },
                    &UnavailableCredentialRevoker,
                )
                .is_err()
        );
        fs::remove_file(repository.data_dir(account.id).join("mail")).expect("clear data state");
        fs::create_dir(repository.account_dir(account.id).join("locks"))
            .expect("unexpected account-local state");
        assert!(
            repository
                .remove_account(
                    "personal",
                    RemoveOptions {
                        discard_local_data: false,
                        keep_credentials: false,
                        revoke_credentials: false,
                    },
                    &UnavailableCredentialRevoker,
                )
                .is_err()
        );
        repository
            .remove_account(
                "personal",
                RemoveOptions {
                    discard_local_data: true,
                    keep_credentials: false,
                    revoke_credentials: false,
                },
                &UnavailableCredentialRevoker,
            )
            .expect("explicit discard");
        assert!(!repository.data_dir(account.id).exists());
    }

    #[test]
    fn credential_choice_is_explicit_and_failed_revocation_preserves_account() {
        let (_directory, repository) = repository();
        repository
            .create_account(NewAccount {
                alias: "personal",
                provider: "gmail",
                provider_identity: Some("person@example.com"),
                credential_profile: Some("google-default"),
            })
            .expect("credential-bound account");

        let no_choice = RemoveOptions {
            discard_local_data: false,
            keep_credentials: false,
            revoke_credentials: false,
        };
        assert!(
            repository
                .remove_account("personal", no_choice, &UnavailableCredentialRevoker)
                .is_err()
        );
        let revoke = RemoveOptions {
            revoke_credentials: true,
            ..no_choice
        };
        assert!(
            repository
                .remove_account("personal", revoke, &UnavailableCredentialRevoker)
                .is_err()
        );
        assert!(repository.account_by_alias("personal").is_ok());

        repository
            .remove_account(
                "personal",
                RemoveOptions {
                    keep_credentials: true,
                    ..no_choice
                },
                &UnavailableCredentialRevoker,
            )
            .expect("explicitly retain credentials");
    }

    #[test]
    fn account_locks_isolate_accounts_concurrently_and_report_contention() {
        let (_directory, repository) = repository();
        let first = add_account(&repository, "first", "first@example.com");
        let second = add_account(&repository, "second", "second@example.com");

        let first_lock = repository.account_lock(first.id).expect("first lock");
        let knowledge_lock = repository
            .knowledge_lock()
            .expect("independent knowledge lock");
        let (second_lock, error) = thread::scope(|scope| {
            let second_lock = scope.spawn(|| repository.account_lock(second.id));
            let contention = scope.spawn(|| repository.account_lock(first.id));
            (
                second_lock
                    .join()
                    .expect("second-account thread")
                    .expect("independent account lock"),
                contention
                    .join()
                    .expect("same-account thread")
                    .expect_err("same account must contend"),
            )
        });
        assert!(error.to_string().contains("pid="));
        assert!(error.to_string().contains("possibly stale lock"));

        drop((first_lock, second_lock, knowledge_lock));
        repository.account_lock(first.id).expect("released lock");
    }

    #[test]
    fn lifecycle_lock_guards_config_and_account_changes() {
        let (_directory, repository) = repository();
        add_account(&repository, "personal", "person@example.com");
        let lifecycle_lock = repository.account_lifecycle_lock().expect("lifecycle lock");

        let errors = thread::scope(|scope| {
            scope
                .spawn(|| {
                    [
                        repository
                            .create_account(NewAccount {
                                alias: "work",
                                provider: "gmail",
                                provider_identity: Some("work@example.com"),
                                credential_profile: None,
                            })
                            .expect_err("create must contend")
                            .to_string(),
                        repository
                            .rename_account("personal", "private")
                            .expect_err("rename must contend")
                            .to_string(),
                        repository
                            .remove_account(
                                "personal",
                                RemoveOptions {
                                    discard_local_data: false,
                                    keep_credentials: false,
                                    revoke_credentials: false,
                                },
                                &UnavailableCredentialRevoker,
                            )
                            .expect_err("remove must contend")
                            .to_string(),
                        repository
                            .set_config("pull.default-limit", "1000")
                            .expect_err("config update must contend")
                            .to_string(),
                        repository
                            .add_oauth_profile(OAuthClientProfile {
                                id: Uuid::new_v4(),
                                alias: "google".into(),
                                provider: "google".into(),
                                client_id: "id".into(),
                            })
                            .expect_err("OAuth profile update must contend")
                            .to_string(),
                    ]
                })
                .join()
                .expect("lifecycle contender")
        });

        assert!(
            errors
                .iter()
                .all(|error| error.contains("account-lifecycle.lock"))
        );
        drop(lifecycle_lock);
        assert!(repository.account_by_alias("personal").is_ok());
        repository
            .set_config("pull.default-limit", "1000")
            .expect("config update after lock release");
        repository
            .add_oauth_profile(OAuthClientProfile {
                id: Uuid::new_v4(),
                alias: "google".into(),
                provider: "google".into(),
                client_id: "id".into(),
            })
            .expect("OAuth profile update after lock release");
        let config = repository.config().expect("preserved config updates");
        assert_eq!(config.pull.default_limit, 1000);
        assert_eq!(config.oauth_clients.len(), 1);
    }

    #[test]
    fn removal_cannot_delete_or_recreate_a_held_account_lock() {
        let (_directory, repository) = repository();
        let account = add_account(&repository, "personal", "person@example.com");
        let lock = repository.account_lock(account.id).expect("account lock");

        let error = thread::scope(|scope| {
            scope
                .spawn(|| {
                    repository.remove_account(
                        "personal",
                        RemoveOptions {
                            discard_local_data: false,
                            keep_credentials: false,
                            revoke_credentials: false,
                        },
                        &UnavailableCredentialRevoker,
                    )
                })
                .join()
                .expect("removal contender")
                .expect_err("held account must not be removable")
        });

        assert!(error.to_string().contains("pid="));
        assert!(repository.account_by_alias("personal").is_ok());
        drop(lock);
        repository
            .remove_account(
                "personal",
                RemoveOptions {
                    discard_local_data: false,
                    keep_credentials: false,
                    revoke_credentials: false,
                },
                &UnavailableCredentialRevoker,
            )
            .expect("removal after lock release");
        assert!(repository.account_lock(account.id).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn init_applies_private_unix_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let (_directory, repository) = repository();
        let account = add_account(&repository, "personal", "person@example.com");
        let account_lock = repository.account_lock(account.id).expect("account lock");
        let knowledge_lock = repository.knowledge_lock().expect("knowledge lock");
        let lifecycle_lock = repository.account_lifecycle_lock().expect("lifecycle lock");
        let mode = |path: &Path| {
            fs::metadata(path)
                .expect("private path metadata")
                .permissions()
                .mode()
                & 0o777
        };

        for path in [
            repository.root().join(".bit-mail"),
            repository.root().join(".bit-mail/accounts"),
            repository.root().join(".bit-mail/locks"),
            repository.root().join(".bit-mail/locks/accounts"),
            repository.root().join("data"),
            repository.root().join("knowledge"),
            repository.root().join("knowledge/global"),
            repository.root().join("knowledge/accounts"),
            repository.root().join("skills"),
            repository.account_dir(account.id),
            repository.data_dir(account.id),
        ] {
            assert_eq!(mode(&path), 0o700, "directory: {}", path.display());
        }
        for path in [
            repository.root().join(".bit-mail/repository.toml"),
            repository.root().join(".bit-mail/config.toml"),
            repository.root().join("AGENTS.md"),
            repository.account_dir(account.id).join("account.toml"),
            repository
                .root()
                .join(format!(".bit-mail/locks/accounts/{}.lock", account.id)),
            repository.root().join(".bit-mail/locks/knowledge.lock"),
            repository
                .root()
                .join(".bit-mail/locks/account-lifecycle.lock"),
        ] {
            assert_eq!(mode(&path), 0o600, "file: {}", path.display());
        }
        for (path, _) in crate::runtime_assets::ASSETS {
            assert_eq!(
                mode(&repository.root().join(path)),
                0o600,
                "runtime asset: {path}"
            );
        }

        drop((account_lock, knowledge_lock, lifecycle_lock));
    }

    #[test]
    fn discovery_restores_an_interrupted_runtime_asset_update() {
        let (_directory, repository) = repository();
        let transaction = repository
            .root()
            .join(".bit-mail/runtime-assets-update-interrupted");
        let backup = transaction.join("old");
        fs::create_dir_all(&backup).unwrap();
        fs::copy(
            repository.root().join(".bit-mail/repository.toml"),
            backup.join("repository.toml"),
        )
        .unwrap();
        fs::copy(
            repository
                .root()
                .join(".bit-mail/integrity/repository.json"),
            backup.join("integrity-manifest.json"),
        )
        .unwrap();
        fs::rename(repository.root().join("skills"), backup.join("skills")).unwrap();
        fs::rename(
            repository.root().join("AGENTS.md"),
            backup.join("AGENTS.md"),
        )
        .unwrap();
        fs::create_dir_all(repository.root().join("skills")).unwrap();
        fs::write(repository.root().join("skills/incomplete"), "incomplete").unwrap();
        fs::write(repository.root().join("AGENTS.md"), "incomplete").unwrap();

        let recovered = Repository::discover_from(repository.root()).unwrap();
        assert!(!transaction.exists());
        assert!(
            crate::integrity::validate_repository(&recovered)
                .unwrap()
                .is_empty()
        );
        for (path, bytes) in crate::runtime_assets::ASSETS {
            assert_eq!(fs::read(recovered.root().join(path)).unwrap(), *bytes);
        }
    }

    #[test]
    fn recovery_removes_partial_assets_from_an_interrupted_legacy_update() {
        let (_directory, repository) = repository();
        fs::remove_file(repository.root().join("AGENTS.md")).unwrap();
        fs::remove_dir_all(repository.root().join("skills")).unwrap();
        let mut metadata = repository.metadata.clone();
        metadata.runtime_assets_version = None;
        write_toml_atomic(
            &repository.root().join(".bit-mail/repository.toml"),
            &metadata,
        )
        .unwrap();
        crate::integrity::commit_repository(&repository).unwrap();

        let transaction = repository
            .root()
            .join(".bit-mail/runtime-assets-update-interrupted");
        let backup = transaction.join("old");
        fs::create_dir_all(&backup).unwrap();
        fs::copy(
            repository.root().join(".bit-mail/repository.toml"),
            backup.join("repository.toml"),
        )
        .unwrap();
        fs::copy(
            repository
                .root()
                .join(".bit-mail/integrity/repository.json"),
            backup.join("integrity-manifest.json"),
        )
        .unwrap();
        for (path, bytes) in crate::runtime_assets::ASSETS {
            let path = repository.root().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }

        let mut interrupted = Repository::open(repository.root().to_path_buf()).unwrap();
        interrupted.recover_runtime_asset_updates().unwrap();
        assert!(!interrupted.root().join("AGENTS.md").exists());
        assert!(!interrupted.root().join("skills").exists());
        assert!(
            crate::integrity::validate_repository(&interrupted)
                .unwrap()
                .is_empty()
        );

        let synchronized = Repository::discover_from(repository.root()).unwrap();
        assert!(
            crate::integrity::validate_repository(&synchronized)
                .unwrap()
                .is_empty()
        );
    }
}
