use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    Result,
    audit::{self, Details},
    provider::MailProvider,
    repository::{AccountConfig, Repository},
    storage::CanonicalStore,
    triage,
};

#[derive(Debug, Serialize)]
pub struct RepairReport {
    pub schema_version: u32,
    pub message_id: Uuid,
    pub thread_messages: usize,
    pub pending: usize,
}

#[derive(Debug, Serialize)]
pub struct GcReport {
    pub schema_version: u32,
    pub dry_run: bool,
    pub threads: usize,
    pub messages: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct CacheRebuildReport {
    pub schema_version: u32,
    pub account_id: Uuid,
}

pub fn index_rebuild(repository: &Repository, account: &AccountConfig) -> Result<()> {
    index_rebuild_with_progress(repository, account, &crate::progress::none)
}

pub fn index_rebuild_with_progress(
    repository: &Repository,
    account: &AccountConfig,
    progress: crate::progress::Reporter<'_>,
) -> Result<()> {
    crate::progress::phase(progress, "Validating account data");
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account(repository, account.id)?;
    crate::progress::phase(progress, "Rebuilding structural index");
    CanonicalStore::new(repository, account)?.rebuild_index_unlocked()
}

#[derive(Deserialize)]
struct ThreadManifest {
    messages: Vec<Uuid>,
}

#[derive(Deserialize)]
struct ProviderThreadRecord {
    provider_thread_id: String,
}

pub fn repair<F>(
    repository: &Repository,
    account: &AccountConfig,
    message_id: Uuid,
    provider: F,
) -> Result<RepairReport>
where
    F: FnOnce() -> Result<Box<dyn MailProvider>>,
{
    repair_with_progress(
        repository,
        account,
        message_id,
        provider,
        &crate::progress::none,
    )
}

pub fn repair_with_progress<F>(
    repository: &Repository,
    account: &AccountConfig,
    message_id: Uuid,
    provider: F,
    progress: crate::progress::Reporter<'_>,
) -> Result<RepairReport>
where
    F: FnOnce() -> Result<Box<dyn MailProvider>>,
{
    crate::progress::phase(progress, "Checking local message state");
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::bootstrap_account(repository, account.id)?;
    crate::integrity::validate_repair_basis(repository, account.id, message_id)?;
    let store = CanonicalStore::new(repository, account)?;
    let (mut affected, context_valid) = match store.context_ids(message_id) {
        Ok(ids) => (ids, true),
        Err(_) => (vec![message_id], false),
    };
    let provider_id = store.identity_provider_message_id(message_id)?;
    crate::progress::phase(progress, "Fetching provider thread");
    let provider = provider()?;
    let reference = provider.message_ref(&provider_id)?;
    let thread = provider.thread(&reference.thread_id)?;
    affected.extend(local_thread_ids(
        &account_dir(repository, account.id).join("provider/messages"),
        &thread.provider_thread_id,
    )?);
    affected.sort_unstable();
    affected.dedup();
    crate::integrity::validate_unaffected_repair_state(repository, account.id, &affected)?;
    if !context_valid {
        remove_file(
            &account_dir(repository, account.id)
                .join("threads")
                .join(format!("{message_id}.json")),
        )?;
    }
    crate::progress::phase(progress, "Rebuilding local thread");
    let ids = store.replace_thread_unlocked(&thread)?;
    for id in affected.iter().filter(|id| !ids.contains(id)) {
        remove_message_cache(repository, account.id, *id)?;
    }
    affected.extend(&ids);
    affected.sort_unstable();
    affected.dedup();
    let mut pending = 0;
    for id in &affected {
        triage::remove_work_item(repository, account.id, *id)?;
    }
    for (message, id) in thread.messages.iter().zip(&ids) {
        if message.flags.inbox && message.flags.unread {
            pending += usize::from(triage::write_pending(repository, account.id, *id)?);
        }
    }
    audit::append(
        &account_dir(repository, account.id).join("audit"),
        "repair",
        Details {
            account_id: Some(account.id),
            message_ids: &affected,
            selection: None,
            knowledge_id: None,
            value: None,
        },
    )?;
    crate::progress::phase(progress, "Finalizing repaired thread");
    crate::integrity::commit_account(repository, account.id)?;
    Ok(RepairReport {
        schema_version: 1,
        message_id,
        thread_messages: ids.len(),
        pending,
    })
}

pub fn gc(repository: &Repository, account: &AccountConfig, dry_run: bool) -> Result<GcReport> {
    gc_with_progress(repository, account, dry_run, &crate::progress::none)
}

pub fn gc_with_progress(
    repository: &Repository,
    account: &AccountConfig,
    dry_run: bool,
    progress: crate::progress::Reporter<'_>,
) -> Result<GcReport> {
    crate::progress::phase(progress, "Scanning provider cache");
    let _lock = repository.account_lock(account.id)?;
    if dry_run {
        let mismatches = crate::integrity::validate_account(repository, account.id)?;
        if let Some(mismatch) = mismatches.first() {
            return Err(io::Error::other(format!(
                "integrity mismatch: {} ({})",
                mismatch.path, mismatch.kind
            ))
            .into());
        }
    } else {
        crate::integrity::prepare_account(repository, account.id)?;
    }
    if !dry_run {
        crate::progress::phase(progress, "Removing unreachable cache data");
    }
    let report = gc_unlocked(repository, account, dry_run, true, None)?;
    if !dry_run {
        crate::integrity::commit_account(repository, account.id)?;
    }
    Ok(report)
}

pub(crate) fn gc_after_push_unlocked(
    repository: &Repository,
    account: &AccountConfig,
    candidates: &HashSet<Uuid>,
) -> Result<GcReport> {
    gc_unlocked(repository, account, false, false, Some(candidates))
}

fn gc_unlocked(
    repository: &Repository,
    account: &AccountConfig,
    dry_run: bool,
    audit_gc: bool,
    candidates: Option<&HashSet<Uuid>>,
) -> Result<GcReport> {
    let account_root = account_dir(repository, account.id);
    let work = uuid_filenames(&account_root.join("work-items"))?;
    let mut manifests = Vec::new();
    let mut reachable = work.clone();
    for path in sorted_json(&account_root.join("threads"))? {
        let manifest: ThreadManifest = serde_json::from_slice(&fs::read(&path)?)?;
        if manifest.messages.iter().any(|id| work.contains(id)) {
            reachable.extend(&manifest.messages);
        } else if candidates.is_none_or(|ids| manifest.messages.iter().any(|id| ids.contains(id))) {
            manifests.push(path);
        }
    }
    let mut cached = uuid_directories(&repository.data_dir(account.id).join("messages"))?;
    cached.extend(uuid_files(&account_root.join("provider/messages"), "json")?);
    cached.extend(uuid_files(&account_root.join("provider/raw"), "eml")?);
    cached.extend(uuid_files(&account_root.join("diagnostics"), "json")?);
    let mut messages = cached.difference(&reachable).copied().collect::<Vec<_>>();
    if let Some(candidates) = candidates {
        messages.retain(|id| candidates.contains(id));
    }
    messages.sort_unstable();
    messages.dedup();
    let report = GcReport {
        schema_version: 1,
        dry_run,
        threads: manifests.len(),
        messages: messages.clone(),
    };
    if dry_run {
        return Ok(report);
    }
    for path in manifests {
        fs::remove_file(path)?;
    }
    for id in &messages {
        remove_message_cache(repository, account.id, *id)?;
    }
    triage::prune_selection_members_unlocked(repository, account.id, &messages)?;
    CanonicalStore::new(repository, account)?.rebuild_index_unlocked()?;
    if audit_gc {
        audit::append(
            &account_root.join("audit"),
            "gc",
            Details {
                account_id: Some(account.id),
                message_ids: &messages,
                selection: None,
                knowledge_id: None,
                value: None,
            },
        )?;
    }
    Ok(report)
}

pub fn cache_rebuild(
    repository: &Repository,
    account: &AccountConfig,
) -> Result<CacheRebuildReport> {
    cache_rebuild_with_progress(repository, account, &crate::progress::none)
}

pub fn cache_rebuild_with_progress(
    repository: &Repository,
    account: &AccountConfig,
    progress: crate::progress::Reporter<'_>,
) -> Result<CacheRebuildReport> {
    crate::progress::phase(progress, "Validating cache rebuild");
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::bootstrap_account(repository, account.id)?;
    crate::integrity::validate_cache_rebuild_guard(repository, account.id)?;
    if triage::staged(repository, account.id)? {
        return Err(io::Error::other(
            "cache rebuild refuses while read/delete work is staged; push or unstage first",
        )
        .into());
    }
    let account_root = account_dir(repository, account.id);
    crate::progress::phase(progress, "Clearing provider cache");
    for path in [
        repository.data_dir(account.id),
        account_root.join("provider"),
        account_root.join("threads"),
        account_root.join("work-items"),
        account_root.join("selections"),
        account_root.join("diagnostics"),
        account_root.join("staging"),
        account_root.join("integrity"),
    ] {
        remove_dir(&path)?;
    }
    remove_file(&account_root.join("provider-state.json"))?;
    remove_file(&account_root.join("index.sqlite"))?;
    fs::create_dir_all(repository.data_dir(account.id))?;
    #[cfg(unix)]
    fs::set_permissions(
        repository.data_dir(account.id),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;
    crate::progress::phase(progress, "Rebuilding index and integrity");
    CanonicalStore::new(repository, account)?.rebuild_index_unlocked()?;
    audit::append(
        &account_root.join("audit"),
        "cache.rebuild",
        Details {
            account_id: Some(account.id),
            message_ids: &[],
            selection: None,
            knowledge_id: None,
            value: None,
        },
    )?;
    crate::integrity::reset_account(repository, account.id)?;
    Ok(CacheRebuildReport {
        schema_version: 1,
        account_id: account.id,
    })
}

fn account_dir(repository: &Repository, account_id: Uuid) -> PathBuf {
    repository
        .root()
        .join(".bit-mail/accounts")
        .join(account_id.to_string())
}

fn sorted_json(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    paths.retain(|path| path.extension().and_then(|value| value.to_str()) == Some("json"));
    paths.sort();
    Ok(paths)
}

fn uuid_filenames(directory: &Path) -> Result<HashSet<Uuid>> {
    Ok(sorted_json(directory)?
        .into_iter()
        .filter_map(|path| path.file_stem()?.to_str()?.parse().ok())
        .collect())
}

fn uuid_files(directory: &Path, extension: &str) -> Result<HashSet<Uuid>> {
    if !directory.is_dir() {
        return Ok(HashSet::new());
    }
    Ok(fs::read_dir(directory)?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.extension()?.to_str()? != extension {
                return None;
            }
            path.file_stem()?.to_str()?.parse().ok()
        })
        .collect())
}

fn uuid_directories(directory: &Path) -> Result<HashSet<Uuid>> {
    if !directory.is_dir() {
        return Ok(HashSet::new());
    }
    Ok(fs::read_dir(directory)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if !entry.path().is_dir() {
                return None;
            }
            entry.file_name().to_str()?.parse().ok()
        })
        .collect())
}

fn local_thread_ids(directory: &Path, provider_thread_id: &str) -> Result<Vec<Uuid>> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut ids = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let Some(id) = path
            .file_stem()
            .and_then(|value| value.to_str())
            .and_then(|value| value.parse::<Uuid>().ok())
        else {
            continue;
        };
        let Ok(record) = serde_json::from_slice::<ProviderThreadRecord>(&fs::read(&path)?) else {
            continue;
        };
        if record.provider_thread_id == provider_thread_id {
            ids.push(id);
        }
    }
    ids.sort_unstable();
    Ok(ids)
}

fn remove_message_cache(repository: &Repository, account_id: Uuid, id: Uuid) -> Result<()> {
    let root = account_dir(repository, account_id);
    remove_dir(
        &repository
            .data_dir(account_id)
            .join("messages")
            .join(id.to_string()),
    )?;
    remove_file(&root.join("provider/messages").join(format!("{id}.json")))?;
    remove_file(&root.join("provider/raw").join(format!("{id}.eml")))?;
    remove_file(&root.join("diagnostics").join(format!("{id}.json")))
}

fn remove_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn remove_dir(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        provider::{HistoryPage, MessageRef, MessageState, Page},
        repository::{GitIgnorePolicy, NewAccount},
        storage::{MailboxFlags, MessageInput, MimePartInput, ThreadInput, TransferEncoding},
        triage::WorkState,
    };

    fn fixture() -> (tempfile::TempDir, Repository, AccountConfig, ThreadInput) {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let account = repository
            .create_account(NewAccount {
                alias: "mail",
                provider: "gmail",
                provider_identity: None,
                credential_profile: None,
            })
            .unwrap();
        let thread = ThreadInput {
            provider: "gmail".into(),
            provider_thread_id: "thread".into(),
            messages: vec![message("one", true), message("two", false)],
        };
        (directory, repository, account, thread)
    }

    fn message(id: &str, actionable: bool) -> MessageInput {
        MessageInput {
            provider_message_id: id.into(),
            provider_thread_id: "thread".into(),
            received_at_ms: 1,
            sent_at_ms: None,
            subject: None,
            from: vec![],
            to: vec![],
            cc: vec![],
            bcc: vec![],
            reply_to: vec![],
            rfc_message_id: None,
            flags: MailboxFlags {
                inbox: actionable,
                unread: actionable,
                ..Default::default()
            },
            parts: vec![MimePartInput {
                id: "0".into(),
                mime_type: "text/plain".into(),
                headers: Default::default(),
                filename: None,
                transfer_encoding: TransferEncoding::None,
                body: Some(id.as_bytes().to_vec()),
                remote: None,
                parts: vec![],
            }],
            provider_source: serde_json::json!({"id": id}),
        }
    }

    struct Fake(ThreadInput);
    impl MailProvider for Fake {
        fn current_history_id(&self) -> Result<String> {
            Ok("1".into())
        }
        fn unread_page(&self, _: Option<&str>, _: u32) -> Result<Page<MessageRef>> {
            unreachable!()
        }
        fn history_page(&self, _: &str, _: Option<&str>) -> Result<HistoryPage> {
            unreachable!()
        }
        fn message_state(&self, _: &str) -> Result<MessageState> {
            Ok(MessageState::Actionable)
        }
        fn message_ref(&self, id: &str) -> Result<MessageRef> {
            Ok(MessageRef {
                id: id.into(),
                thread_id: "thread".into(),
            })
        }
        fn thread(&self, _: &str) -> Result<ThreadInput> {
            Ok(self.0.clone())
        }
        fn attachment(&self, _: &str, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
        fn raw(&self, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
    }

    #[test]
    fn repair_replaces_thread_and_invalidates_staged_intent() {
        let (_directory, repository, account, thread) = fixture();
        let ids = CanonicalStore::new(&repository, &account)
            .unwrap()
            .materialize_thread(&thread)
            .unwrap();
        triage::write_pending(&repository, account.id, ids[0]).unwrap();
        triage::stage(&repository, &account, &[ids[0]], WorkState::Delete).unwrap();
        fs::write(
            CanonicalStore::new(&repository, &account)
                .unwrap()
                .message_path(ids[0])
                .join("metadata.json"),
            "corrupt provider-derived metadata",
        )
        .unwrap();
        fs::write(
            account_dir(&repository, account.id)
                .join("threads")
                .join(format!("{}.json", ids[0])),
            "corrupt thread manifest",
        )
        .unwrap();
        let authoritative = ThreadInput {
            messages: vec![thread.messages[0].clone()],
            ..thread
        };
        let report = repair(&repository, &account, ids[0], || {
            Ok(Box::new(Fake(authoritative)))
        })
        .unwrap();
        assert_eq!((report.thread_messages, report.pending), (1, 1));
        let work = triage::work_items(&repository, &account, None).unwrap();
        assert_eq!(work.work_items[0].state, WorkState::Pending);
        assert!(
            !CanonicalStore::new(&repository, &account)
                .unwrap()
                .message_path(ids[1])
                .exists()
        );
    }

    #[test]
    fn gc_retains_shared_thread_until_last_work_item_is_gone() {
        let (_directory, repository, account, thread) = fixture();
        let ids = CanonicalStore::new(&repository, &account)
            .unwrap()
            .materialize_thread(&thread)
            .unwrap();
        let raw = account_dir(&repository, account.id)
            .join("provider/raw")
            .join(format!("{}.eml", ids[0]));
        fs::create_dir_all(raw.parent().unwrap()).unwrap();
        fs::write(&raw, "raw").unwrap();
        let diagnostic = account_dir(&repository, account.id)
            .join("diagnostics")
            .join(format!("{}.json", ids[0]));
        fs::write(&diagnostic, "diagnostic").unwrap();
        crate::integrity::commit_account(&repository, account.id).unwrap();
        triage::write_pending(&repository, account.id, ids[0]).unwrap();
        let manifest = account_dir(&repository, account.id).join("integrity/manifest.json");
        let manifest_before = fs::read(&manifest).unwrap();
        let audit_before =
            sorted_json(&account_dir(&repository, account.id).join("audit")).unwrap();
        assert_eq!(gc(&repository, &account, true).unwrap().threads, 0);
        assert_eq!(fs::read(&manifest).unwrap(), manifest_before);
        assert_eq!(
            sorted_json(&account_dir(&repository, account.id).join("audit")).unwrap(),
            audit_before
        );
        triage::remove_work_item(&repository, account.id, ids[0]).unwrap();
        assert_eq!(gc(&repository, &account, true).unwrap().messages, ids);
        let provider_record = account_dir(&repository, account.id)
            .join("provider/messages")
            .join(format!("{}.json", ids[0]));
        assert!(provider_record.is_file());
        gc(&repository, &account, false).unwrap();
        assert!(
            !CanonicalStore::new(&repository, &account)
                .unwrap()
                .message_path(ids[0])
                .exists()
        );
        assert!(!provider_record.exists());
        assert!(!raw.exists());
        assert!(!diagnostic.exists());
    }

    #[test]
    fn cache_rebuild_preserves_identity_knowledge_and_audit() {
        let (_directory, repository, account, thread) = fixture();
        let store = CanonicalStore::new(&repository, &account).unwrap();
        let first = store.materialize_thread(&thread).unwrap();
        let knowledge = crate::knowledge::add(&repository, Some(&account), "remember").unwrap();
        crate::triage::create_selection(&repository, &account, "review").unwrap();
        let content_path = store.message_path(first[0]).join("content.md");
        let selection_path = account_dir(&repository, account.id).join("selections/review.json");
        let content = fs::read(&content_path).unwrap();
        let knowledge_bytes = fs::read(&knowledge.path).unwrap();
        let selection = fs::read(&selection_path).unwrap();
        fs::write(&content_path, "tampered message").unwrap();
        fs::write(&knowledge.path, "tampered Knowledge").unwrap();
        fs::write(&selection_path, "tampered selection").unwrap();
        let mismatches = crate::integrity::validate_account(&repository, account.id).unwrap();
        assert!(
            mismatches
                .iter()
                .any(|item| item.path.ends_with("content.md"))
        );
        assert!(
            mismatches
                .iter()
                .any(|item| item.path.ends_with(&format!("{}.md", knowledge.id)))
        );
        assert!(
            mismatches
                .iter()
                .any(|item| item.path.ends_with("review.json"))
        );
        fs::write(&content_path, content).unwrap();
        fs::write(&knowledge.path, knowledge_bytes).unwrap();
        fs::write(&selection_path, selection).unwrap();
        triage::write_pending(&repository, account.id, first[0]).unwrap();
        triage::stage(&repository, &account, &[first[0]], WorkState::Read).unwrap();
        let work_path = account_dir(&repository, account.id)
            .join("work-items")
            .join(format!("{}.json", first[0]));
        let work = fs::read(&work_path).unwrap();
        fs::write(&work_path, "tampered work item").unwrap();
        assert!(
            crate::integrity::validate_account(&repository, account.id)
                .unwrap()
                .iter()
                .any(|item| item.path.ends_with(&format!("{}.json", first[0])))
        );
        fs::write(&work_path, work).unwrap();
        assert!(cache_rebuild(&repository, &account).is_err());
        triage::unstage(&repository, &account, &[first[0]]).unwrap();
        fs::write(&content_path, "corrupt disposable cache").unwrap();
        cache_rebuild(&repository, &account).unwrap();
        let second = store.materialize_thread(&thread).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            crate::knowledge::show(&repository, Some(&account), knowledge.id)
                .unwrap()
                .content
                .as_deref(),
            Some("remember\n")
        );
        assert!(
            account_dir(&repository, account.id)
                .join("audit")
                .read_dir()
                .unwrap()
                .next()
                .is_some()
        );
    }

    #[test]
    fn cache_rebuild_fails_closed_when_staged_intent_is_untrustworthy() {
        let (_directory, repository, account, thread) = fixture();
        let store = CanonicalStore::new(&repository, &account).unwrap();
        let ids = store.materialize_thread(&thread).unwrap();
        triage::write_pending(&repository, account.id, ids[0]).unwrap();
        triage::stage(&repository, &account, &[ids[0]], WorkState::Read).unwrap();
        let work_path = account_dir(&repository, account.id)
            .join("work-items")
            .join(format!("{}.json", ids[0]));
        let work = fs::read(&work_path).unwrap();

        let mut pending: serde_json::Value = serde_json::from_slice(&work).unwrap();
        pending["state"] = serde_json::Value::String("pending".into());
        fs::write(&work_path, serde_json::to_vec_pretty(&pending).unwrap()).unwrap();
        let error = cache_rebuild(&repository, &account).unwrap_err();
        assert!(error.to_string().contains("integrity mismatch"));
        assert!(store.message_path(ids[0]).is_dir());
        assert!(work_path.is_file());

        fs::write(&work_path, work).unwrap();
        fs::remove_file(&work_path).unwrap();
        let error = cache_rebuild(&repository, &account).unwrap_err();
        assert!(error.to_string().contains("integrity mismatch"));
        assert!(store.message_path(ids[0]).is_dir());
    }
}
