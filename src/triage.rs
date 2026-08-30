use std::{
    collections::HashSet,
    fmt, fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Result,
    audit::{self, Details},
    repository::{AccountConfig, Repository},
    storage::CanonicalStore,
};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    Pending,
    Read,
    Delete,
}

impl fmt::Display for WorkState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pending => "pending",
            Self::Read => "read",
            Self::Delete => "delete",
        })
    }
}

impl FromStr for WorkState {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "read" => Ok(Self::Read),
            "delete" => Ok(Self::Delete),
            _ => Err("state must be pending, read, or delete".into()),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkItem {
    pub(crate) schema_version: u32,
    pub(crate) message_id: Uuid,
    pub(crate) state: WorkState,
}

#[derive(Debug, Serialize)]
pub struct MessageReference {
    pub message_id: Uuid,
    pub content_path: PathBuf,
    pub metadata_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct WorkItemView {
    pub message_id: Uuid,
    pub state: WorkState,
    pub content_path: PathBuf,
    pub metadata_path: PathBuf,
    pub context: Vec<MessageReference>,
}

#[derive(Debug, Serialize)]
pub struct WorkItemsOutput {
    pub schema_version: u32,
    pub account_id: Uuid,
    pub account_alias: String,
    pub work_items: Vec<WorkItemView>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub schema_version: u32,
    pub account_id: Uuid,
    pub name: String,
    pub message_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct StagedItem {
    pub message_id: Uuid,
    pub state: WorkState,
}

pub fn work_items(
    repository: &Repository,
    account: &AccountConfig,
    filter: Option<WorkState>,
) -> Result<WorkItemsOutput> {
    let store = CanonicalStore::new(repository, account)?;
    let mut views = Vec::new();
    for (_, item) in read_work_items(&work_items_dir(repository, account.id))? {
        if filter.is_some_and(|state| state != item.state) {
            continue;
        }
        let message_path = store.message_path(item.message_id);
        let context = store
            .context_ids(item.message_id)?
            .into_iter()
            .map(|message_id| {
                let path = store.message_path(message_id);
                MessageReference {
                    message_id,
                    content_path: path.join("content.md"),
                    metadata_path: path.join("metadata.json"),
                }
            })
            .collect();
        views.push(WorkItemView {
            message_id: item.message_id,
            state: item.state,
            content_path: message_path.join("content.md"),
            metadata_path: message_path.join("metadata.json"),
            context,
        });
    }
    views.sort_by_key(|item| item.message_id);
    Ok(WorkItemsOutput {
        schema_version: SCHEMA_VERSION,
        account_id: account.id,
        account_alias: account.alias.clone(),
        work_items: views,
    })
}

pub fn stage(
    repository: &Repository,
    account: &AccountConfig,
    ids: &[Uuid],
    state: WorkState,
) -> Result<usize> {
    validate_stage_state(state)?;
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account_triage(repository, account.id)?;
    let changed = stage_unlocked(repository, account, ids, state)?;
    crate::integrity::commit_account_triage(repository, account.id)?;
    Ok(changed)
}

pub fn stage_selection(
    repository: &Repository,
    account: &AccountConfig,
    name: &str,
    state: WorkState,
) -> Result<usize> {
    validate_stage_state(state)?;
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account_triage(repository, account.id)?;
    let selection = read_selection(repository, account.id, name)?;
    if selection.message_ids.is_empty() {
        return Ok(0);
    }
    let changed = stage_unlocked(repository, account, &selection.message_ids, state)?;
    crate::integrity::commit_account_triage(repository, account.id)?;
    Ok(changed)
}

fn stage_unlocked(
    repository: &Repository,
    account: &AccountConfig,
    ids: &[Uuid],
    state: WorkState,
) -> Result<usize> {
    let ids = normalized_ids(ids)?;
    let directory = work_items_dir(repository, account.id);
    let mut items = Vec::new();
    for id in &ids {
        let item = read_work_item(&directory, *id)?;
        if item.state != WorkState::Pending && item.state != state {
            return Err(error(format!(
                "work item {id} is staged {}; unstage it first",
                item.state
            )));
        }
        items.push(item);
    }
    let changed: Vec<_> = items
        .iter_mut()
        .filter_map(|item| {
            if item.state == state {
                None
            } else {
                item.state = state;
                Some(item.message_id)
            }
        })
        .collect();
    for item in &items {
        if changed.contains(&item.message_id) {
            write_json_atomic(&directory.join(format!("{}.json", item.message_id)), item)?;
        }
    }
    if !changed.is_empty() {
        audit::append(
            &audit_dir(repository, account.id),
            "stage",
            Details {
                account_id: Some(account.id),
                message_ids: &changed,
                selection: None,
                knowledge_id: None,
                value: Some(&state.to_string()),
            },
        )?;
    }
    Ok(changed.len())
}

pub fn unstage(repository: &Repository, account: &AccountConfig, ids: &[Uuid]) -> Result<usize> {
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account_triage(repository, account.id)?;
    let changed = unstage_unlocked(repository, account, ids)?;
    crate::integrity::commit_account_triage(repository, account.id)?;
    Ok(changed)
}

pub fn unstage_selection(
    repository: &Repository,
    account: &AccountConfig,
    name: &str,
) -> Result<usize> {
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account_triage(repository, account.id)?;
    let selection = read_selection(repository, account.id, name)?;
    if selection.message_ids.is_empty() {
        return Ok(0);
    }
    let changed = unstage_unlocked(repository, account, &selection.message_ids)?;
    crate::integrity::commit_account_triage(repository, account.id)?;
    Ok(changed)
}

fn unstage_unlocked(
    repository: &Repository,
    account: &AccountConfig,
    ids: &[Uuid],
) -> Result<usize> {
    let ids = normalized_ids(ids)?;
    let directory = work_items_dir(repository, account.id);
    let mut items = ids
        .iter()
        .map(|id| read_work_item(&directory, *id))
        .collect::<Result<Vec<_>>>()?;
    let changed: Vec<_> = items
        .iter_mut()
        .filter_map(|item| {
            if item.state == WorkState::Pending {
                None
            } else {
                item.state = WorkState::Pending;
                Some(item.message_id)
            }
        })
        .collect();
    for item in &items {
        if changed.contains(&item.message_id) {
            write_json_atomic(&directory.join(format!("{}.json", item.message_id)), item)?;
        }
    }
    if !changed.is_empty() {
        audit::append(
            &audit_dir(repository, account.id),
            "unstage",
            Details {
                account_id: Some(account.id),
                message_ids: &changed,
                selection: None,
                knowledge_id: None,
                value: None,
            },
        )?;
    }
    Ok(changed.len())
}

pub fn create_selection(
    repository: &Repository,
    account: &AccountConfig,
    name: &str,
) -> Result<Selection> {
    validate_selection_name(name)?;
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account_triage(repository, account.id)?;
    let path = selection_path(repository, account.id, name);
    if path.exists() {
        return Err(error(format!("selection already exists: {name}")));
    }
    let selection = Selection {
        schema_version: SCHEMA_VERSION,
        account_id: account.id,
        name: name.into(),
        message_ids: Vec::new(),
    };
    write_json_atomic(&path, &selection)?;
    audit_selection(repository, account, "selection.create", &selection, &[])?;
    crate::integrity::commit_account_triage(repository, account.id)?;
    Ok(selection)
}

pub fn add_selection(
    repository: &Repository,
    account: &AccountConfig,
    name: &str,
    ids: &[Uuid],
) -> Result<Selection> {
    let ids = normalized_ids(ids)?;
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account_triage(repository, account.id)?;
    let mut selection = read_selection(repository, account.id, name)?;
    for id in &ids {
        read_work_item(&work_items_dir(repository, account.id), *id)?;
    }
    let before: HashSet<_> = selection.message_ids.iter().copied().collect();
    selection.message_ids.extend(&ids);
    selection.message_ids.sort_unstable();
    selection.message_ids.dedup();
    let changed: Vec<_> = selection
        .message_ids
        .iter()
        .filter(|id| !before.contains(id))
        .copied()
        .collect();
    if !changed.is_empty() {
        write_json_atomic(&selection_path(repository, account.id, name), &selection)?;
        audit_selection(repository, account, "selection.add", &selection, &changed)?;
    }
    crate::integrity::commit_account_triage(repository, account.id)?;
    Ok(selection)
}

pub fn remove_selection_members(
    repository: &Repository,
    account: &AccountConfig,
    name: &str,
    ids: &[Uuid],
) -> Result<Selection> {
    let ids: HashSet<_> = normalized_ids(ids)?.into_iter().collect();
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account_triage(repository, account.id)?;
    let mut selection = read_selection(repository, account.id, name)?;
    let changed: Vec<_> = selection
        .message_ids
        .iter()
        .filter(|id| ids.contains(id))
        .copied()
        .collect();
    selection.message_ids.retain(|id| !ids.contains(id));
    if !changed.is_empty() {
        write_json_atomic(&selection_path(repository, account.id, name), &selection)?;
        audit_selection(
            repository,
            account,
            "selection.remove",
            &selection,
            &changed,
        )?;
    }
    crate::integrity::commit_account_triage(repository, account.id)?;
    Ok(selection)
}

pub fn show_selection(
    repository: &Repository,
    account: &AccountConfig,
    name: &str,
) -> Result<Selection> {
    read_selection(repository, account.id, name)
}

pub fn delete_selection(
    repository: &Repository,
    account: &AccountConfig,
    name: &str,
) -> Result<Selection> {
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account_triage(repository, account.id)?;
    let selection = read_selection(repository, account.id, name)?;
    fs::remove_file(selection_path(repository, account.id, name))?;
    audit_selection(
        repository,
        account,
        "selection.delete",
        &selection,
        &selection.message_ids,
    )?;
    crate::integrity::commit_account_triage(repository, account.id)?;
    Ok(selection)
}

pub(crate) fn staged(repository: &Repository, account_id: Uuid) -> Result<bool> {
    Ok(read_work_items(&work_items_dir(repository, account_id))?
        .iter()
        .any(|(_, item)| item.state != WorkState::Pending))
}

pub(crate) fn staged_items(repository: &Repository, account_id: Uuid) -> Result<Vec<StagedItem>> {
    Ok(read_work_items(&work_items_dir(repository, account_id))?
        .into_iter()
        .filter_map(|(_, item)| {
            (item.state != WorkState::Pending).then_some(StagedItem {
                message_id: item.message_id,
                state: item.state,
            })
        })
        .collect())
}

pub(crate) fn selection_ids(
    repository: &Repository,
    account_id: Uuid,
    name: &str,
) -> Result<Vec<Uuid>> {
    Ok(read_selection(repository, account_id, name)?.message_ids)
}

pub(crate) fn write_pending(
    repository: &Repository,
    account_id: Uuid,
    message_id: Uuid,
) -> Result<bool> {
    let directory = work_items_dir(repository, account_id);
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{message_id}.json"));
    if path.exists() {
        return Ok(false);
    }
    write_json_atomic(
        &path,
        &WorkItem {
            schema_version: SCHEMA_VERSION,
            message_id,
            state: WorkState::Pending,
        },
    )?;
    crate::integrity::commit_account_triage(repository, account_id)?;
    Ok(true)
}

pub(crate) fn remove_work_item(
    repository: &Repository,
    account_id: Uuid,
    message_id: Uuid,
) -> Result<bool> {
    let removed = remove_work_item_unlocked(repository, account_id, message_id)?;
    if removed {
        crate::integrity::commit_account_triage(repository, account_id)?;
    }
    Ok(removed)
}

pub(crate) fn remove_work_item_unlocked(
    repository: &Repository,
    account_id: Uuid,
    message_id: Uuid,
) -> Result<bool> {
    let path = work_items_dir(repository, account_id).join(format!("{message_id}.json"));
    match fs::remove_file(path) {
        Ok(()) => {
            prune_selection_member(repository, account_id, message_id)?;
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn prune_pending(
    repository: &Repository,
    account_id: Uuid,
    active: &HashSet<Uuid>,
) -> Result<usize> {
    let mut removed = 0;
    for (_, item) in read_work_items(&work_items_dir(repository, account_id))? {
        if item.state == WorkState::Pending && !active.contains(&item.message_id) {
            removed += usize::from(remove_work_item(repository, account_id, item.message_id)?);
        }
    }
    Ok(removed)
}

fn normalized_ids(ids: &[Uuid]) -> Result<Vec<Uuid>> {
    if ids.is_empty() {
        return Err(error("at least one message ID is required"));
    }
    let mut ids = ids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

fn validate_stage_state(state: WorkState) -> Result<()> {
    if state == WorkState::Pending {
        return Err(error(
            "stage requires read or delete; use unstage for pending",
        ));
    }
    Ok(())
}

fn read_work_item(directory: &Path, id: Uuid) -> Result<WorkItem> {
    let path = directory.join(format!("{id}.json"));
    let item: WorkItem = serde_json::from_slice(&fs::read(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            io::Error::other(format!("unknown work item: {id}"))
        } else {
            error
        }
    })?)?;
    if item.schema_version != SCHEMA_VERSION || item.message_id != id {
        return Err(error(format!("invalid work item: {id}")));
    }
    Ok(item)
}

fn read_work_items(directory: &Path) -> Result<Vec<(PathBuf, WorkItem)>> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    paths.sort();
    paths
        .into_iter()
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .map(|path| {
            let item: WorkItem = serde_json::from_slice(&fs::read(&path)?)?;
            let id = path
                .file_stem()
                .and_then(|value| value.to_str())
                .and_then(|value| value.parse::<Uuid>().ok());
            if item.schema_version != SCHEMA_VERSION || id != Some(item.message_id) {
                return Err(error("invalid work-item schema or identity"));
            }
            Ok((path, item))
        })
        .collect()
}

fn read_selection(repository: &Repository, account_id: Uuid, name: &str) -> Result<Selection> {
    validate_selection_name(name)?;
    let selection: Selection =
        serde_json::from_slice(&fs::read(selection_path(repository, account_id, name))?)?;
    if selection.schema_version != SCHEMA_VERSION
        || selection.account_id != account_id
        || selection.name != name
    {
        return Err(error("invalid selection schema or identity"));
    }
    Ok(selection)
}

fn prune_selection_member(
    repository: &Repository,
    account_id: Uuid,
    message_id: Uuid,
) -> Result<()> {
    let directory = selections_dir(repository, account_id);
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let mut selection: Selection = serde_json::from_slice(&fs::read(&path)?)?;
        if selection.schema_version != SCHEMA_VERSION || selection.account_id != account_id {
            return Err(error("invalid selection schema or account"));
        }
        let length = selection.message_ids.len();
        selection.message_ids.retain(|id| *id != message_id);
        if selection.message_ids.len() != length {
            write_json_atomic(&path, &selection)?;
            audit::append(
                &audit_dir(repository, account_id),
                "selection.prune",
                Details {
                    account_id: Some(account_id),
                    message_ids: &[message_id],
                    selection: Some(&selection.name),
                    knowledge_id: None,
                    value: None,
                },
            )?;
        }
    }
    Ok(())
}

pub(crate) fn prune_selection_members_unlocked(
    repository: &Repository,
    account_id: Uuid,
    message_ids: &[Uuid],
) -> Result<()> {
    for message_id in message_ids {
        prune_selection_member(repository, account_id, *message_id)?;
    }
    Ok(())
}

fn audit_selection(
    repository: &Repository,
    account: &AccountConfig,
    action: &str,
    selection: &Selection,
    ids: &[Uuid],
) -> Result<()> {
    audit::append(
        &audit_dir(repository, account.id),
        action,
        Details {
            account_id: Some(account.id),
            message_ids: ids,
            selection: Some(&selection.name),
            knowledge_id: None,
            value: None,
        },
    )
}

fn validate_selection_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 32
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
        || !name.as_bytes()[0].is_ascii_alphanumeric()
        || !name.as_bytes()[name.len() - 1].is_ascii_alphanumeric()
    {
        return Err(error(
            "selection names must be 1-32 lowercase letters, digits, '-' or '_', starting and ending alphanumeric",
        ));
    }
    Ok(())
}

fn work_items_dir(repository: &Repository, account_id: Uuid) -> PathBuf {
    repository
        .root()
        .join(".bit-mail/accounts")
        .join(account_id.to_string())
        .join("work-items")
}

fn selections_dir(repository: &Repository, account_id: Uuid) -> PathBuf {
    repository
        .root()
        .join(".bit-mail/accounts")
        .join(account_id.to_string())
        .join("selections")
}

fn selection_path(repository: &Repository, account_id: Uuid, name: &str) -> PathBuf {
    selections_dir(repository, account_id).join(format!("{name}.json"))
}

fn audit_dir(repository: &Repository, account_id: Uuid) -> PathBuf {
    repository
        .root()
        .join(".bit-mail/accounts")
        .join(account_id.to_string())
        .join("audit")
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        set_private_directory(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    set_private_file(&temporary)?;
    fs::rename(temporary, path)?;
    Ok(())
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

    fn repository() -> (tempfile::TempDir, Repository, AccountConfig) {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let account = repository
            .create_account(NewAccount {
                alias: "personal",
                provider: "gmail",
                provider_identity: Some("person@example.com"),
                credential_profile: None,
            })
            .unwrap();
        (directory, repository, account)
    }

    #[test]
    fn bulk_validation_precedes_mutation_and_selections_prune_on_resolution() {
        let (_directory, repository, account) = repository();
        let first = Uuid::now_v7();
        let missing = Uuid::now_v7();
        write_pending(&repository, account.id, first).unwrap();

        assert!(stage(&repository, &account, &[first, missing], WorkState::Read).is_err());
        assert_eq!(
            read_work_item(&work_items_dir(&repository, account.id), first)
                .unwrap()
                .state,
            WorkState::Pending
        );

        assert_eq!(
            stage(&repository, &account, &[first], WorkState::Read).unwrap(),
            1
        );
        assert_eq!(
            stage(&repository, &account, &[first], WorkState::Read).unwrap(),
            0
        );
        assert!(stage(&repository, &account, &[first], WorkState::Delete).is_err());
        assert_eq!(unstage(&repository, &account, &[first]).unwrap(), 1);

        create_selection(&repository, &account, "review").unwrap();
        add_selection(&repository, &account, "review", &[first, first]).unwrap();
        assert_eq!(
            show_selection(&repository, &account, "review")
                .unwrap()
                .message_ids,
            [first]
        );
        let other = repository
            .create_account(NewAccount {
                alias: "work",
                provider: "gmail",
                provider_identity: Some("work@example.com"),
                credential_profile: None,
            })
            .unwrap();
        create_selection(&repository, &other, "review").unwrap();
        assert!(add_selection(&repository, &other, "review", &[first]).is_err());
        remove_work_item(&repository, account.id, first).unwrap();
        assert!(
            show_selection(&repository, &account, "review")
                .unwrap()
                .message_ids
                .is_empty()
        );

        write_pending(&repository, account.id, missing).unwrap();
        let lock = repository.account_lock(account.id).unwrap();
        assert!(
            stage(&repository, &account, &[missing], WorkState::Read)
                .unwrap_err()
                .to_string()
                .contains("lock")
        );
        drop(lock);
    }

    #[test]
    fn pending_is_only_reached_through_unstage() {
        let (_directory, repository, account) = repository();
        let id = Uuid::now_v7();
        write_pending(&repository, account.id, id).unwrap();

        let assert_pending_rejected = |result: Result<usize>| {
            assert_eq!(
                result.unwrap_err().to_string(),
                "stage requires read or delete; use unstage for pending"
            );
        };
        assert_pending_rejected(stage(&repository, &account, &[id], WorkState::Pending));
        create_selection(&repository, &account, "empty").unwrap();
        assert_pending_rejected(stage_selection(
            &repository,
            &account,
            "empty",
            WorkState::Pending,
        ));
        create_selection(&repository, &account, "review").unwrap();
        add_selection(&repository, &account, "review", &[id]).unwrap();
        assert_pending_rejected(stage_selection(
            &repository,
            &account,
            "review",
            WorkState::Pending,
        ));
        assert_eq!(
            read_work_item(&work_items_dir(&repository, account.id), id)
                .unwrap()
                .state,
            WorkState::Pending
        );
    }

    #[test]
    fn work_item_json_references_canonical_message_and_ordered_context() {
        let (_directory, repository, account) = repository();
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        write_pending(&repository, account.id, first).unwrap();
        let account_dir = repository
            .root()
            .join(".bit-mail/accounts")
            .join(account.id.to_string());
        fs::create_dir_all(account_dir.join("threads")).unwrap();
        fs::write(
            account_dir.join("threads/thread.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "provider": "gmail",
                "provider_thread_id": "thread",
                "messages": [second, first]
            }))
            .unwrap(),
        )
        .unwrap();

        let output = work_items(&repository, &account, Some(WorkState::Pending)).unwrap();
        assert_eq!(output.work_items[0].message_id, first);
        assert_eq!(
            output.work_items[0]
                .context
                .iter()
                .map(|item| item.message_id)
                .collect::<Vec<_>>(),
            [second, first]
        );
        assert!(output.work_items[0].content_path.ends_with("content.md"));
    }
}
