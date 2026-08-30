use std::{
    collections::{BTreeSet, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[cfg(test)]
use crate::triage::{WorkItem, WorkState};
use crate::{
    Result,
    provider::{MailProvider, MessageState, ProviderError, ProviderErrorKind},
    repository::{AccountConfig, Repository},
    storage::{AttachmentState, CanonicalStore},
    triage,
};

const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy)]
pub struct PullOptions {
    pub limit: u32,
    pub all: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Blocked,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct AccountReport {
    pub account_id: Uuid,
    pub alias: String,
    pub outcome: Outcome,
    pub seeds: usize,
    pub threads: usize,
    pub additional_unread: usize,
    pub new_work_items: usize,
    pub removed_work_items: usize,
    pub retries: Option<u32>,
    pub backlog_remaining: Option<bool>,
    pub history_fallback: bool,
    pub failures: usize,
}

#[derive(Debug, Serialize)]
pub struct PullReport {
    pub schema_version: u32,
    pub accounts: Vec<AccountReport>,
}

impl PullReport {
    pub fn new(accounts: Vec<AccountReport>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            accounts,
        }
    }
    pub fn failed(&self) -> bool {
        self.accounts
            .iter()
            .any(|v| !matches!(v.outcome, Outcome::Success))
    }
}

pub fn failed_account_report(account: &AccountConfig) -> AccountReport {
    let mut value = report(account, Outcome::Failed);
    value.failures = 1;
    value
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderState {
    schema_version: u32,
    history_id: Option<String>,
    backlog_page_token: Option<String>,
    backlog_remaining: bool,
    last_successful_pull_ms: Option<u64>,
    #[serde(default)]
    last_successful_push_ms: Option<u64>,
}

pub fn pull_account<F>(
    repository: &Repository,
    account: &AccountConfig,
    options: PullOptions,
    provider: F,
) -> Result<AccountReport>
where
    F: FnOnce() -> Result<Box<dyn MailProvider>>,
{
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account(repository, account.id)?;
    let paths = Paths::new(repository, account.id);
    create_private_dir(&paths.work_items)?;
    if triage::staged(repository, account.id)? {
        let mut result = report(account, Outcome::Blocked);
        result.retries = Some(0);
        return Ok(result);
    }
    let provider = provider()?;
    let store = CanonicalStore::new(repository, account)?;
    let mut state = read_state(&paths.provider_state)?;
    let initial = state.history_id.is_none();
    let mut next_history = if initial {
        provider.current_history_id()?
    } else {
        state.history_id.clone().unwrap()
    };
    let mut thread_ids = BTreeSet::new();
    let mut seed_ids = HashSet::new();
    let mut removed = 0;
    let mut fallback = false;

    if !initial {
        let mut page = None;
        loop {
            match provider.history_page(state.history_id.as_deref().unwrap(), page.as_deref()) {
                Ok(history) => {
                    next_history = history.history_id;
                    for changed in history.changed {
                        match provider.message_state(&changed.id)? {
                            MessageState::Actionable => {
                                seed_ids.insert(changed.id);
                                thread_ids.insert(changed.thread_id);
                            }
                            MessageState::Inactive => {
                                if let Some(id) = store.message_id_for_provider(&changed.id)? {
                                    removed += usize::from(triage::remove_work_item(
                                        repository, account.id, id,
                                    )?);
                                    thread_ids.insert(changed.thread_id);
                                }
                            }
                            MessageState::Missing => {
                                if let Some(id) = store.message_id_for_provider(&changed.id)? {
                                    removed += usize::from(triage::remove_work_item(
                                        repository, account.id, id,
                                    )?);
                                }
                            }
                        }
                    }
                    page = history.next_page;
                    if page.is_none() {
                        break;
                    }
                }
                Err(error)
                    if error
                        .downcast_ref::<ProviderError>()
                        .is_some_and(|v| v.0 == ProviderErrorKind::HistoryExpired) =>
                {
                    fallback = true;
                    next_history = provider.current_history_id()?;
                    break;
                }
                Err(error) => return Err(error),
            }
        }
    }

    let unlimited = options.all || fallback;
    let mut remaining = if unlimited { u32::MAX } else { options.limit };
    let mut page = if unlimited {
        None
    } else {
        state.backlog_page_token.clone()
    };
    let mut next_backlog = None;
    let mut restarted_backlog = false;
    while (initial || unlimited || state.backlog_remaining) && remaining > 0 {
        let size = remaining.min(500);
        let values = match provider.unread_page(page.as_deref(), size) {
            Ok(values) => values,
            Err(_) if page.is_some() && !restarted_backlog => {
                page = None;
                restarted_backlog = true;
                continue;
            }
            Err(error) => return Err(error),
        };
        for message in values.items {
            seed_ids.insert(message.id);
            thread_ids.insert(message.thread_id);
        }
        remaining = remaining.saturating_sub(size);
        next_backlog = values.next_page;
        if next_backlog.is_none() || !unlimited && remaining == 0 {
            break;
        }
        page.clone_from(&next_backlog);
    }

    let fetched = fetch_threads(provider.as_ref(), thread_ids.iter().cloned().collect());
    let mut result = report(account, Outcome::Success);
    result.seeds = seed_ids.len();
    result.threads = fetched.len();
    result.removed_work_items = removed;
    let mut active = HashSet::new();
    for fetched in fetched {
        match fetched {
            Ok(thread) => {
                let ids = store.materialize_thread_unlocked(&thread)?;
                for (message, id) in thread.messages.iter().zip(ids) {
                    if message.flags.inbox && message.flags.unread {
                        active.insert(id);
                        result.additional_unread +=
                            usize::from(!seed_ids.contains(&message.provider_message_id));
                        result.new_work_items +=
                            usize::from(triage::write_pending(repository, account.id, id)?);
                    }
                }
            }
            Err(_) => result.failures += 1,
        }
    }
    if fallback && result.failures == 0 {
        result.removed_work_items += triage::prune_pending(repository, account.id, &active)?;
    }
    result.retries = Some(provider.retries());
    result.history_fallback = fallback;
    let backlog_remaining = next_backlog.is_some();
    result.backlog_remaining = Some(backlog_remaining);
    if result.failures == 0 {
        state = ProviderState {
            schema_version: SCHEMA_VERSION,
            history_id: Some(next_history),
            backlog_page_token: next_backlog,
            backlog_remaining,
            last_successful_pull_ms: Some(
                SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64,
            ),
            last_successful_push_ms: state.last_successful_push_ms,
        };
        write_json_atomic(&paths.provider_state, &state)?;
    } else {
        result.outcome = Outcome::Failed;
    }
    crate::integrity::commit_account(repository, account.id)?;
    Ok(result)
}

pub(crate) fn record_successful_push(repository: &Repository, account_id: Uuid) -> Result<u64> {
    let path = Paths::new(repository, account_id).provider_state;
    let mut state = read_state(&path)?;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as u64;
    state.schema_version = SCHEMA_VERSION;
    state.last_successful_push_ms = Some(timestamp);
    write_json_atomic(&path, &state)?;
    Ok(timestamp)
}

pub fn fetch_attachment<F>(
    repository: &Repository,
    account: &AccountConfig,
    message_id: Uuid,
    part_id: &str,
    provider: F,
) -> Result<PathBuf>
where
    F: FnOnce() -> Result<Box<dyn MailProvider>>,
{
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account(repository, account.id)?;
    let store = CanonicalStore::new(repository, account)?;
    match store.attachment_state(message_id, part_id)? {
        AttachmentState::Local { path } => Ok(path),
        AttachmentState::Remote(remote) => {
            let provider_id = store.provider_message_id(message_id)?;
            let bytes = provider()?.attachment(&provider_id, &remote.provider_attachment_id)?;
            let path = store.persist_attachment_unlocked(message_id, part_id, &bytes)?;
            crate::integrity::commit_account(repository, account.id)?;
            Ok(path)
        }
    }
}

pub fn fetch_raw<F>(
    repository: &Repository,
    account: &AccountConfig,
    message_id: Uuid,
    provider: F,
) -> Result<PathBuf>
where
    F: FnOnce() -> Result<Box<dyn MailProvider>>,
{
    let _lock = repository.account_lock(account.id)?;
    crate::integrity::prepare_account(repository, account.id)?;
    let store = CanonicalStore::new(repository, account)?;
    let path = store.raw_path(message_id);
    if path.is_file() {
        return Ok(path);
    }
    let provider_id = store.provider_message_id(message_id)?;
    let bytes = provider()?.raw(&provider_id)?;
    let path = store.persist_raw_unlocked(message_id, &bytes)?;
    crate::integrity::commit_account(repository, account.id)?;
    Ok(path)
}

fn fetch_threads(
    provider: &dyn MailProvider,
    ids: Vec<String>,
) -> Vec<Result<crate::storage::ThreadInput>> {
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(ids.len()));
    std::thread::scope(|scope| {
        for _ in 0..ids.len().min(4) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(id) = ids.get(index) else {
                        break;
                    };
                    results.lock().unwrap().push((index, provider.thread(id)));
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|v| v.0);
    results.into_iter().map(|v| v.1).collect()
}

struct Paths {
    provider_state: PathBuf,
    work_items: PathBuf,
}
impl Paths {
    fn new(repository: &Repository, id: Uuid) -> Self {
        let root = repository
            .root()
            .join(".bit-mail/accounts")
            .join(id.to_string());
        Self {
            provider_state: root.join("provider-state.json"),
            work_items: root.join("work-items"),
        }
    }
}
fn report(account: &AccountConfig, outcome: Outcome) -> AccountReport {
    AccountReport {
        account_id: account.id,
        alias: account.alias.clone(),
        outcome,
        seeds: 0,
        threads: 0,
        additional_unread: 0,
        new_work_items: 0,
        removed_work_items: 0,
        retries: None,
        backlog_remaining: None,
        history_fallback: false,
        failures: 0,
    }
}
fn read_state(path: &Path) -> Result<ProviderState> {
    if !path.exists() {
        return Ok(ProviderState {
            schema_version: SCHEMA_VERSION,
            ..Default::default()
        });
    }
    let state: ProviderState = serde_json::from_slice(&fs::read(path)?)?;
    if state.schema_version != SCHEMA_VERSION {
        return Err(io::Error::other("unsupported provider state schema").into());
    }
    Ok(state)
}

pub fn validate_provider_state(repository: &Repository, account_id: Uuid) -> Result<()> {
    read_state(&Paths::new(repository, account_id).provider_state).map(|_| ())
}
fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(value)?)?;
    set_private_file(&temporary)?;
    fs::rename(temporary, path)?;
    Ok(())
}
fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn set_private_file(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        provider::{HistoryPage, MessageRef, Page},
        repository::{AccountConfig, GitIgnorePolicy, NewAccount},
        storage::{
            CanonicalMetadata, MailboxFlags, MessageInput, MimePartInput, ThreadInput,
            TransferEncoding,
        },
    };
    use std::{collections::BTreeMap, sync::Mutex};

    struct Fake {
        pages: Mutex<Vec<Option<String>>>,
    }

    struct FailedThread;
    impl MailProvider for FailedThread {
        fn current_history_id(&self) -> Result<String> {
            Ok("10".into())
        }
        fn unread_page(&self, _: Option<&str>, _: u32) -> Result<Page<MessageRef>> {
            Ok(Page {
                items: vec![
                    MessageRef {
                        id: "a".into(),
                        thread_id: "good".into(),
                    },
                    MessageRef {
                        id: "x".into(),
                        thread_id: "bad".into(),
                    },
                ],
                next_page: None,
            })
        }
        fn history_page(&self, _: &str, _: Option<&str>) -> Result<HistoryPage> {
            unreachable!()
        }
        fn message_state(&self, _: &str) -> Result<MessageState> {
            unreachable!()
        }
        fn thread(&self, id: &str) -> Result<ThreadInput> {
            if id == "bad" {
                Err(io::Error::other("failed thread").into())
            } else {
                Ok(ThreadInput {
                    provider: "gmail".into(),
                    provider_thread_id: id.into(),
                    messages: vec![Fake::message("a", id)],
                })
            }
        }
        fn attachment(&self, _: &str, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
        fn raw(&self, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
    }

    struct Expired;
    impl MailProvider for Expired {
        fn current_history_id(&self) -> Result<String> {
            Ok("20".into())
        }
        fn unread_page(&self, _: Option<&str>, _: u32) -> Result<Page<MessageRef>> {
            Ok(Page {
                items: vec![MessageRef {
                    id: "a".into(),
                    thread_id: "t".into(),
                }],
                next_page: None,
            })
        }
        fn history_page(&self, _: &str, _: Option<&str>) -> Result<HistoryPage> {
            Err(ProviderError(ProviderErrorKind::HistoryExpired, "expired").into())
        }
        fn message_state(&self, _: &str) -> Result<MessageState> {
            unreachable!()
        }
        fn thread(&self, id: &str) -> Result<ThreadInput> {
            Ok(ThreadInput {
                provider: "gmail".into(),
                provider_thread_id: id.into(),
                messages: vec![Fake::message("a", id)],
            })
        }
        fn attachment(&self, _: &str, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
        fn raw(&self, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
    }

    struct Changed(MessageState);
    impl MailProvider for Changed {
        fn current_history_id(&self) -> Result<String> {
            unreachable!()
        }
        fn unread_page(&self, _: Option<&str>, _: u32) -> Result<Page<MessageRef>> {
            panic!("completed backlog must not restart")
        }
        fn history_page(&self, _: &str, _: Option<&str>) -> Result<HistoryPage> {
            Ok(HistoryPage {
                changed: vec![MessageRef {
                    id: "a".into(),
                    thread_id: "t".into(),
                }],
                next_page: None,
                history_id: "12".into(),
            })
        }
        fn message_state(&self, _: &str) -> Result<MessageState> {
            Ok(self.0)
        }
        fn thread(&self, id: &str) -> Result<ThreadInput> {
            assert_eq!(
                self.0,
                MessageState::Inactive,
                "missing messages need no thread"
            );
            let mut inactive = Fake::message("a", id);
            inactive.flags.unread = false;
            Ok(ThreadInput {
                provider: "gmail".into(),
                provider_thread_id: id.into(),
                messages: vec![inactive, Fake::message("b", id)],
            })
        }
        fn attachment(&self, _: &str, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
        fn raw(&self, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
    }

    struct Content;
    impl MailProvider for Content {
        fn current_history_id(&self) -> Result<String> {
            unreachable!()
        }
        fn unread_page(&self, _: Option<&str>, _: u32) -> Result<Page<MessageRef>> {
            unreachable!()
        }
        fn history_page(&self, _: &str, _: Option<&str>) -> Result<HistoryPage> {
            unreachable!()
        }
        fn message_state(&self, _: &str) -> Result<MessageState> {
            unreachable!()
        }
        fn thread(&self, _: &str) -> Result<ThreadInput> {
            unreachable!()
        }
        fn attachment(&self, _: &str, _: &str) -> Result<Vec<u8>> {
            Ok(b"abc".to_vec())
        }
        fn raw(&self, _: &str) -> Result<Vec<u8>> {
            Ok(b"raw".to_vec())
        }
    }

    struct ManyChanged;
    impl MailProvider for ManyChanged {
        fn current_history_id(&self) -> Result<String> {
            unreachable!()
        }
        fn unread_page(&self, _: Option<&str>, _: u32) -> Result<Page<MessageRef>> {
            panic!("completed backlog must not restart")
        }
        fn history_page(&self, _: &str, _: Option<&str>) -> Result<HistoryPage> {
            Ok(HistoryPage {
                changed: vec![
                    MessageRef {
                        id: "a".into(),
                        thread_id: "t".into(),
                    },
                    MessageRef {
                        id: "c".into(),
                        thread_id: "u".into(),
                    },
                ],
                next_page: None,
                history_id: "12".into(),
            })
        }
        fn message_state(&self, _: &str) -> Result<MessageState> {
            Ok(MessageState::Actionable)
        }
        fn thread(&self, id: &str) -> Result<ThreadInput> {
            Ok(ThreadInput {
                provider: "gmail".into(),
                provider_thread_id: id.into(),
                messages: if id == "t" {
                    vec![Fake::message("a", id), Fake::message("b", id)]
                } else {
                    vec![Fake::message("c", id)]
                },
            })
        }
        fn attachment(&self, _: &str, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
        fn raw(&self, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
    }

    struct InvalidThread;
    impl MailProvider for InvalidThread {
        fn current_history_id(&self) -> Result<String> {
            Ok("10".into())
        }
        fn unread_page(&self, _: Option<&str>, _: u32) -> Result<Page<MessageRef>> {
            Ok(Page {
                items: vec![MessageRef {
                    id: "a".into(),
                    thread_id: "t".into(),
                }],
                next_page: None,
            })
        }
        fn history_page(&self, _: &str, _: Option<&str>) -> Result<HistoryPage> {
            unreachable!()
        }
        fn message_state(&self, _: &str) -> Result<MessageState> {
            unreachable!()
        }
        fn thread(&self, id: &str) -> Result<ThreadInput> {
            Ok(ThreadInput {
                provider: "other".into(),
                provider_thread_id: id.into(),
                messages: vec![Fake::message("a", id)],
            })
        }
        fn attachment(&self, _: &str, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
        fn raw(&self, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
    }

    impl Fake {
        fn message(id: &str, thread: &str) -> MessageInput {
            MessageInput {
                provider_message_id: id.into(),
                provider_thread_id: thread.into(),
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
                    inbox: true,
                    unread: true,
                    ..Default::default()
                },
                parts: vec![MimePartInput {
                    id: "0".into(),
                    mime_type: "text/plain".into(),
                    headers: BTreeMap::new(),
                    filename: None,
                    transfer_encoding: TransferEncoding::None,
                    body: Some(b"body".to_vec()),
                    remote: None,
                    parts: vec![],
                }],
                provider_source: serde_json::json!({"id": id}),
            }
        }
    }
    impl MailProvider for Fake {
        fn current_history_id(&self) -> Result<String> {
            Ok("10".into())
        }
        fn unread_page(&self, page: Option<&str>, _limit: u32) -> Result<Page<MessageRef>> {
            self.pages.lock().unwrap().push(page.map(str::to_owned));
            Ok(if page.is_none() {
                Page {
                    items: vec![MessageRef {
                        id: "a".into(),
                        thread_id: "t".into(),
                    }],
                    next_page: Some("older".into()),
                }
            } else {
                Page {
                    items: vec![],
                    next_page: None,
                }
            })
        }
        fn history_page(&self, _start: &str, _page: Option<&str>) -> Result<HistoryPage> {
            Ok(HistoryPage {
                changed: vec![],
                next_page: None,
                history_id: "11".into(),
            })
        }
        fn message_state(&self, _id: &str) -> Result<MessageState> {
            Ok(MessageState::Actionable)
        }
        fn thread(&self, _id: &str) -> Result<ThreadInput> {
            Ok(ThreadInput {
                provider: "gmail".into(),
                provider_thread_id: "t".into(),
                messages: vec![Self::message("a", "t"), Self::message("b", "t")],
            })
        }
        fn attachment(&self, _: &str, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
        fn raw(&self, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
    }

    fn pulled_repository() -> (tempfile::TempDir, Repository, AccountConfig, Uuid) {
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
        pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 1,
                all: true,
            },
            || {
                Ok(Box::new(Fake {
                    pages: Mutex::new(vec![]),
                }))
            },
        )
        .unwrap();
        let id = CanonicalStore::new(&repository, &account)
            .unwrap()
            .message_id_for_provider("a")
            .unwrap()
            .unwrap();
        (directory, repository, account, id)
    }

    #[test]
    fn pull_persists_complete_thread_work_and_resumes_old_backlog() {
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
        let fake = Fake {
            pages: Mutex::new(vec![]),
        };
        let first = pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 1,
                all: false,
            },
            || Ok(Box::new(fake)),
        )
        .unwrap();
        assert_eq!(
            (first.seeds, first.additional_unread, first.new_work_items),
            (1, 1, 2)
        );
        assert_eq!(first.retries, Some(0));
        assert_eq!(first.backlog_remaining, Some(true));
        assert_eq!(
            fs::read_dir(Paths::new(&repository, account.id).work_items)
                .unwrap()
                .count(),
            2
        );
        let fake = Fake {
            pages: Mutex::new(vec![]),
        };
        let second = pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 1,
                all: false,
            },
            || Ok(Box::new(fake)),
        )
        .unwrap();
        assert_eq!(
            second.seeds, 0,
            "the saved page token must skip the newest seed page"
        );
        let third = pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 1,
                all: false,
            },
            || {
                Ok(Box::new(Fake {
                    pages: Mutex::new(vec![]),
                }))
            },
        )
        .unwrap();
        assert_eq!(
            third.seeds, 0,
            "an exhausted backlog must rely on history instead of restarting"
        );
    }

    #[test]
    fn staged_work_refuses_before_provider_construction() {
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
        let paths = Paths::new(&repository, account.id);
        fs::create_dir_all(&paths.work_items).unwrap();
        let id = Uuid::new_v4();
        write_json_atomic(
            &paths.work_items.join(format!("{id}.json")),
            &WorkItem {
                schema_version: 1,
                message_id: id,
                state: WorkState::Read,
            },
        )
        .unwrap();
        crate::integrity::commit_account(&repository, account.id).unwrap();
        let report = pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 1,
                all: false,
            },
            || panic!("provider must not be constructed"),
        )
        .unwrap();
        assert!(matches!(report.outcome, Outcome::Blocked));
        assert_eq!(report.retries, Some(0));
        assert_eq!(report.backlog_remaining, None);
    }

    #[test]
    fn backlog_limit_does_not_defer_incremental_history() {
        let (_directory, repository, account, _) = pulled_repository();

        let report = pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 1,
                all: false,
            },
            || Ok(Box::new(ManyChanged)),
        )
        .unwrap();

        assert_eq!((report.seeds, report.threads), (2, 2));
        assert_eq!(
            read_state(&Paths::new(&repository, account.id).provider_state)
                .unwrap()
                .history_id
                .as_deref(),
            Some("12"),
            "history must reconcile completely before advancing its cursor"
        );
        let id = CanonicalStore::new(&repository, &account)
            .unwrap()
            .message_id_for_provider("c")
            .unwrap()
            .unwrap();
        assert!(
            Paths::new(&repository, account.id)
                .work_items
                .join(format!("{id}.json"))
                .is_file()
        );
    }

    #[test]
    fn materialization_failure_exposes_no_work_or_checkpoint() {
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

        pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 1,
                all: false,
            },
            || Ok(Box::new(InvalidThread)),
        )
        .unwrap_err();

        let paths = Paths::new(&repository, account.id);
        assert!(!paths.provider_state.exists());
        assert_eq!(fs::read_dir(paths.work_items).unwrap().count(), 0);
    }

    #[test]
    fn failed_thread_keeps_checkpoint_retryable_and_other_threads_complete() {
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
        let report = pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 2,
                all: false,
            },
            || Ok(Box::new(FailedThread)),
        )
        .unwrap();
        let paths = Paths::new(&repository, account.id);
        assert!(matches!(report.outcome, Outcome::Failed));
        assert_eq!(report.failures, 1);
        assert!(
            !paths.provider_state.exists(),
            "failed work must not advance checkpoints"
        );
        assert_eq!(
            fs::read_dir(repository.data_dir(account.id).join("messages"))
                .unwrap()
                .count(),
            1,
            "only the complete thread may publish"
        );
    }

    #[test]
    fn expired_history_runs_full_reconciliation_and_replaces_cursor() {
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
        pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 1,
                all: false,
            },
            || {
                Ok(Box::new(Fake {
                    pages: Mutex::new(vec![]),
                }))
            },
        )
        .unwrap();
        let report = pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 1,
                all: false,
            },
            || Ok(Box::new(Expired)),
        )
        .unwrap();
        let state = read_state(&Paths::new(&repository, account.id).provider_state).unwrap();
        assert!(report.history_fallback);
        assert_eq!(state.history_id.as_deref(), Some("20"));
        assert!(!state.backlog_remaining);
    }

    #[test]
    fn inactive_history_refreshes_canonical_flags_and_removes_work() {
        let (_directory, repository, account, id) = pulled_repository();

        let report = pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 1,
                all: false,
            },
            || Ok(Box::new(Changed(MessageState::Inactive))),
        )
        .unwrap();

        let metadata: CanonicalMetadata = serde_json::from_slice(
            &fs::read(
                repository
                    .data_dir(account.id)
                    .join("messages")
                    .join(id.to_string())
                    .join("metadata.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(
            !metadata.flags.unread,
            "history refresh must update canonical flags"
        );
        assert!(
            !Paths::new(&repository, account.id)
                .work_items
                .join(format!("{id}.json"))
                .exists()
        );
        assert_eq!(report.removed_work_items, 1);
    }

    #[test]
    fn missing_history_removes_work_without_fetching_a_thread() {
        let (_directory, repository, account, id) = pulled_repository();

        let report = pull_account(
            &repository,
            &account,
            PullOptions {
                limit: 1,
                all: false,
            },
            || Ok(Box::new(Changed(MessageState::Missing))),
        )
        .unwrap();

        assert!(
            !Paths::new(&repository, account.id)
                .work_items
                .join(format!("{id}.json"))
                .exists()
        );
        assert_eq!(report.removed_work_items, 1);
    }

    #[test]
    fn on_demand_content_is_local_and_idempotent_after_the_first_fetch() {
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
        let mut message = Fake::message("a", "t");
        message.parts.push(MimePartInput {
            id: "1".into(),
            mime_type: "application/octet-stream".into(),
            headers: BTreeMap::new(),
            filename: Some("a.bin".into()),
            transfer_encoding: TransferEncoding::None,
            body: None,
            remote: Some(crate::storage::RemoteAttachment {
                provider_attachment_id: "remote".into(),
                size: 3,
            }),
            parts: vec![],
        });
        let id = CanonicalStore::new(&repository, &account)
            .unwrap()
            .materialize_thread(&ThreadInput {
                provider: "gmail".into(),
                provider_thread_id: "t".into(),
                messages: vec![message],
            })
            .unwrap()[0];
        let attachment =
            fetch_attachment(&repository, &account, id, "1", || Ok(Box::new(Content))).unwrap();
        assert_eq!(fs::read(&attachment).unwrap(), b"abc");
        assert_eq!(
            fetch_attachment(&repository, &account, id, "1", || panic!(
                "local attachment must skip provider"
            ))
            .unwrap(),
            attachment
        );
        let raw = fetch_raw(&repository, &account, id, || Ok(Box::new(Content))).unwrap();
        assert_eq!(fs::read(&raw).unwrap(), b"raw");
        assert_eq!(
            fetch_raw(&repository, &account, id, || panic!(
                "local raw must skip provider"
            ))
            .unwrap(),
            raw
        );
    }
}
