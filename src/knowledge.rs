use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Result,
    audit::{self, Details},
    repository::{AccountConfig, Repository},
};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeScope {
    Global,
    Account,
}

impl std::fmt::Display for KnowledgeScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Global => "global",
            Self::Account => "account",
        })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    schema_version: u32,
    id: Uuid,
    scope: KnowledgeScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<Uuid>,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct KnowledgeItem {
    pub id: Uuid,
    pub scope: KnowledgeScope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<Uuid>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct KnowledgeOutput {
    pub schema_version: u32,
    pub knowledge: Vec<KnowledgeItem>,
}

pub fn add(
    repository: &Repository,
    account: Option<&AccountConfig>,
    content: &str,
) -> Result<KnowledgeItem> {
    let content = normalize(content)?;
    match account {
        Some(account) => {
            let _lock = repository.account_lock(account.id)?;
            add_unlocked(repository, Some(account), &content)
        }
        None => {
            let _lock = repository.knowledge_lock()?;
            add_unlocked(repository, None, &content)
        }
    }
}

fn add_unlocked(
    repository: &Repository,
    account: Option<&AccountConfig>,
    content: &str,
) -> Result<KnowledgeItem> {
    let id = Uuid::now_v7();
    let now = now_ms()?;
    let metadata = Frontmatter {
        schema_version: SCHEMA_VERSION,
        id,
        scope: if account.is_some() {
            KnowledgeScope::Account
        } else {
            KnowledgeScope::Global
        },
        account_id: account.map(|value| value.id),
        created_at_ms: now,
        updated_at_ms: now,
    };
    let path = scope_dir(repository, account.map(|value| value.id)).join(format!("{id}.md"));
    write_item(&path, &metadata, content)?;
    audit_change(repository, account, "knowledge.add", id, &metadata.scope)?;
    Ok(item(metadata, path, Some(content.into())))
}

pub fn list(repository: &Repository, account: Option<&AccountConfig>) -> Result<KnowledgeOutput> {
    let mut knowledge = read_dir(&scope_dir(repository, None), false)?;
    if let Some(account) = account {
        knowledge.extend(read_dir(&scope_dir(repository, Some(account.id)), false)?);
    }
    knowledge.sort_by_key(|item| item.id);
    Ok(KnowledgeOutput {
        schema_version: SCHEMA_VERSION,
        knowledge,
    })
}

pub fn show(
    repository: &Repository,
    account: Option<&AccountConfig>,
    id: Uuid,
) -> Result<KnowledgeItem> {
    if let Some(account) = account {
        let path = scope_dir(repository, Some(account.id)).join(format!("{id}.md"));
        if path.exists() {
            return read_item(&path, true);
        }
    }
    read_item(&scope_dir(repository, None).join(format!("{id}.md")), true)
}

pub fn update(
    repository: &Repository,
    account: Option<&AccountConfig>,
    id: Uuid,
    content: &str,
) -> Result<KnowledgeItem> {
    let content = normalize(content)?;
    match account {
        Some(account) => {
            let _lock = repository.account_lock(account.id)?;
            update_unlocked(repository, Some(account), id, &content)
        }
        None => {
            let _lock = repository.knowledge_lock()?;
            update_unlocked(repository, None, id, &content)
        }
    }
}

fn update_unlocked(
    repository: &Repository,
    account: Option<&AccountConfig>,
    id: Uuid,
    content: &str,
) -> Result<KnowledgeItem> {
    let path = scope_dir(repository, account.map(|value| value.id)).join(format!("{id}.md"));
    let mut metadata = read_metadata(&path)?;
    validate_scope(&metadata, account)?;
    metadata.updated_at_ms = now_ms()?;
    write_item(&path, &metadata, content)?;
    audit_change(repository, account, "knowledge.update", id, &metadata.scope)?;
    Ok(item(metadata, path, Some(content.into())))
}

pub fn remove(repository: &Repository, account: Option<&AccountConfig>, id: Uuid) -> Result<()> {
    match account {
        Some(account) => {
            let _lock = repository.account_lock(account.id)?;
            remove_unlocked(repository, Some(account), id)
        }
        None => {
            let _lock = repository.knowledge_lock()?;
            remove_unlocked(repository, None, id)
        }
    }
}

fn remove_unlocked(
    repository: &Repository,
    account: Option<&AccountConfig>,
    id: Uuid,
) -> Result<()> {
    let path = scope_dir(repository, account.map(|value| value.id)).join(format!("{id}.md"));
    let metadata = read_metadata(&path)?;
    validate_scope(&metadata, account)?;
    fs::remove_file(path)?;
    audit_change(repository, account, "knowledge.remove", id, &metadata.scope)
}

fn read_dir(directory: &Path, content: bool) -> Result<Vec<KnowledgeItem>> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    paths.sort();
    paths
        .into_iter()
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("md"))
        .map(|path| read_item(&path, content))
        .collect()
}

fn read_item(path: &Path, include_content: bool) -> Result<KnowledgeItem> {
    let value = fs::read_to_string(path)?;
    let (metadata, content) = parse(&value)?;
    if path.file_stem().and_then(|value| value.to_str()) != Some(&metadata.id.to_string()) {
        return Err(error("Knowledge filename does not match its ID"));
    }
    Ok(item(
        metadata,
        path.into(),
        include_content.then(|| content.into()),
    ))
}

fn read_metadata(path: &Path) -> Result<Frontmatter> {
    Ok(parse(&fs::read_to_string(path)?)?.0)
}

fn parse(value: &str) -> Result<(Frontmatter, &str)> {
    let value = value
        .strip_prefix("+++\n")
        .ok_or_else(|| error("Knowledge item is missing TOML frontmatter"))?;
    let (frontmatter, content) = value
        .split_once("+++\n")
        .ok_or_else(|| error("Knowledge item has unterminated TOML frontmatter"))?;
    let metadata: Frontmatter = toml::from_str(frontmatter)?;
    if metadata.schema_version != SCHEMA_VERSION {
        return Err(error("unsupported Knowledge schema"));
    }
    match (&metadata.scope, metadata.account_id) {
        (KnowledgeScope::Global, None) | (KnowledgeScope::Account, Some(_)) => {}
        _ => return Err(error("Knowledge scope does not match its metadata")),
    }
    Ok((metadata, content))
}

fn write_item(path: &Path, metadata: &Frontmatter, content: &str) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| error("Knowledge path has no parent"))?;
    fs::create_dir_all(parent)?;
    set_private_directory(parent)?;
    let temporary = path.with_extension("md.tmp");
    fs::write(
        &temporary,
        format!("+++\n{}+++\n{content}", toml::to_string_pretty(metadata)?),
    )?;
    set_private_file(&temporary)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn normalize(content: &str) -> Result<String> {
    let content = content.replace("\r\n", "\n").replace('\r', "\n");
    let content = content.trim_end();
    if content.trim().is_empty() {
        return Err(error("Knowledge content must not be empty"));
    }
    Ok(format!("{content}\n"))
}

fn validate_scope(metadata: &Frontmatter, account: Option<&AccountConfig>) -> Result<()> {
    let expected = account.map(|value| value.id);
    if metadata.account_id != expected
        || (expected.is_some()) != (metadata.scope == KnowledgeScope::Account)
    {
        return Err(error("Knowledge scope does not match its location"));
    }
    Ok(())
}

fn audit_change(
    repository: &Repository,
    account: Option<&AccountConfig>,
    action: &str,
    id: Uuid,
    scope: &KnowledgeScope,
) -> Result<()> {
    let directory = match account {
        Some(account) => repository
            .root()
            .join(".bit-mail/accounts")
            .join(account.id.to_string())
            .join("audit"),
        None => repository.root().join(".bit-mail/audit"),
    };
    audit::append(
        &directory,
        action,
        Details {
            account_id: account.map(|value| value.id),
            message_ids: &[],
            selection: None,
            knowledge_id: Some(id),
            value: Some(match scope {
                KnowledgeScope::Global => "global",
                KnowledgeScope::Account => "account",
            }),
        },
    )
}

fn item(metadata: Frontmatter, path: PathBuf, content: Option<String>) -> KnowledgeItem {
    KnowledgeItem {
        id: metadata.id,
        scope: metadata.scope,
        account_id: metadata.account_id,
        created_at_ms: metadata.created_at_ms,
        updated_at_ms: metadata.updated_at_ms,
        path,
        content,
    }
}

fn scope_dir(repository: &Repository, account_id: Option<Uuid>) -> PathBuf {
    match account_id {
        Some(id) => repository
            .root()
            .join("knowledge/accounts")
            .join(id.to_string()),
        None => repository.root().join("knowledge/global"),
    }
}

fn now_ms() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64)
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_directory(_: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_private_file(_: &Path) -> Result<()> {
    Ok(())
}

fn error(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    io::Error::other(message.into()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{GitIgnorePolicy, NewAccount};

    #[test]
    fn global_and_account_knowledge_are_scoped_locked_and_content_redacted_from_audit() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let account = repository
            .create_account(NewAccount {
                alias: "work",
                provider: "gmail",
                provider_identity: Some("work@example.com"),
                credential_profile: None,
            })
            .unwrap();
        let global = add(&repository, None, "global secret preference").unwrap();
        let local = add(&repository, Some(&account), "account secret preference").unwrap();

        assert_eq!(list(&repository, None).unwrap().knowledge.len(), 1);
        assert_eq!(
            list(&repository, Some(&account)).unwrap().knowledge.len(),
            2
        );
        assert!(update(&repository, None, local.id, "wrong scope").is_err());
        assert_eq!(
            show(&repository, Some(&account), global.id)
                .unwrap()
                .content
                .as_deref(),
            Some("global secret preference\n")
        );

        let audits = [
            repository.root().join(".bit-mail/audit"),
            repository
                .root()
                .join(".bit-mail/accounts")
                .join(account.id.to_string())
                .join("audit"),
        ];
        for directory in audits {
            let path = fs::read_dir(directory)
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let value = fs::read_to_string(path).unwrap();
            assert!(!value.contains("secret preference"));
        }

        let lock = repository.knowledge_lock().unwrap();
        assert!(add(&repository, None, "blocked").is_err());
        drop(lock);
        let lock = repository.account_lock(account.id).unwrap();
        assert!(add(&repository, Some(&account), "blocked").is_err());
        drop(lock);
    }
}
