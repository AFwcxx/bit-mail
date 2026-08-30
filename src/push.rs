use std::{
    collections::HashSet,
    io,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use serde::Serialize;
use uuid::Uuid;

use crate::{
    Result,
    audit::{self, Details},
    provider::{MailProvider, ProviderError, ProviderErrorKind, PushMessageState},
    repository::{AccountConfig, Repository},
    storage::CanonicalStore,
    triage::{self, WorkState},
};

const SCHEMA_VERSION: u32 = 1;
const MAX_WORKERS: usize = 4;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum PushScope {
    AllStaged,
    Message(Uuid),
    Selection(String),
}

#[derive(Debug, Clone)]
pub struct PushOptions {
    pub scope: PushScope,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushAction {
    Read,
    Delete,
}

impl PushAction {
    fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemOutcome {
    Planned,
    Mutated,
    AlreadySatisfied,
    Missing,
    Failed,
    NotAttempted,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PushOutcome {
    Preview,
    Cancelled,
    Success,
    PartialFailure,
    AuthenticationFailed,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushItem {
    pub message_id: Uuid,
    pub action: PushAction,
    pub threaded_delete: bool,
    pub outcome: ItemOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<&'static str>,
    #[serde(skip)]
    provider_id: String,
    #[serde(skip)]
    context_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushReport {
    pub schema_version: u32,
    pub account_id: Uuid,
    pub account_alias: String,
    pub scope: PushScope,
    pub dry_run: bool,
    pub outcome: PushOutcome,
    pub retries: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_successful_push_ms: Option<u64>,
    pub items: Vec<PushItem>,
}

impl PushReport {
    pub fn failed(&self) -> bool {
        matches!(
            self.outcome,
            PushOutcome::PartialFailure | PushOutcome::AuthenticationFailed
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewStage {
    Normal,
    ThreadedDelete,
}

pub fn push_account<F, C>(
    repository: &Repository,
    account: &AccountConfig,
    options: PushOptions,
    provider: F,
    mut confirm: C,
) -> Result<PushReport>
where
    F: FnOnce() -> Result<Box<dyn MailProvider>>,
    C: FnMut(ReviewStage, &PushReport) -> Result<bool>,
{
    let _lock = repository.account_lock(account.id)?;
    let staged = scoped_items(repository, account, &options.scope)?;
    let ids = staged
        .iter()
        .map(|item| item.message_id)
        .collect::<Vec<_>>();
    let selection = match &options.scope {
        PushScope::Selection(name) => Some(name.as_str()),
        _ => None,
    };
    let mismatches =
        crate::integrity::validate_sensitive_scope(repository, account.id, &ids, selection)?;
    if let Some(mismatch) = mismatches.first() {
        return Err(io::Error::other(format!(
            "integrity mismatch: {} ({})",
            mismatch.path, mismatch.kind
        ))
        .into());
    }

    let store = CanonicalStore::new(repository, account)?;
    let mut items = Vec::with_capacity(staged.len());
    for staged in staged {
        let action = match staged.state {
            WorkState::Read => PushAction::Read,
            WorkState::Delete => PushAction::Delete,
            WorkState::Pending => unreachable!("pending items are excluded"),
        };
        let context_ids = store.context_ids(staged.message_id)?;
        items.push(PushItem {
            message_id: staged.message_id,
            action,
            threaded_delete: action == PushAction::Delete && context_ids.len() > 1,
            outcome: ItemOutcome::Planned,
            failure_kind: None,
            provider_id: store.provider_message_id(staged.message_id)?,
            context_ids,
        });
    }
    items.sort_by_key(|item| item.message_id);
    let mut report = PushReport {
        schema_version: SCHEMA_VERSION,
        account_id: account.id,
        account_alias: account.alias.clone(),
        scope: options.scope.clone(),
        dry_run: options.dry_run,
        outcome: PushOutcome::Preview,
        retries: 0,
        last_successful_push_ms: None,
        items,
    };
    if options.dry_run || report.items.is_empty() {
        return Ok(report);
    }
    let affected = report
        .items
        .iter()
        .flat_map(|item| item.context_ids.iter().copied())
        .collect::<HashSet<_>>();
    let mismatches = crate::integrity::validate_push_cleanup_scope(
        repository,
        account.id,
        &affected.iter().copied().collect::<Vec<_>>(),
    )?;
    if let Some(mismatch) = mismatches.first() {
        return Err(io::Error::other(format!(
            "integrity mismatch: {} ({})",
            mismatch.path, mismatch.kind
        ))
        .into());
    }
    if !confirm(ReviewStage::Normal, &report)? {
        report.outcome = PushOutcome::Cancelled;
        return Ok(report);
    }
    if report.items.iter().any(|item| item.threaded_delete)
        && !confirm(ReviewStage::ThreadedDelete, &report)?
    {
        report.outcome = PushOutcome::Cancelled;
        return Ok(report);
    }

    let provider = provider()?;
    execute(provider.as_ref(), &mut report.items);
    report.retries = provider.retries();
    let authentication_failed = report
        .items
        .iter()
        .any(|item| item.failure_kind == Some("authentication"));
    let permanent_failed = report
        .items
        .iter()
        .any(|item| item.outcome == ItemOutcome::Failed);
    report.outcome = if authentication_failed {
        PushOutcome::AuthenticationFailed
    } else if permanent_failed {
        PushOutcome::PartialFailure
    } else {
        PushOutcome::Success
    };

    for item in &report.items {
        if matches!(
            item.outcome,
            ItemOutcome::Mutated | ItemOutcome::AlreadySatisfied | ItemOutcome::Missing
        ) {
            triage::remove_work_item_unlocked(repository, account.id, item.message_id)?;
        }
        let value = format!("{}.{}", item.action.label(), outcome_label(item.outcome));
        audit::append(
            &repository
                .root()
                .join(".bit-mail/accounts")
                .join(account.id.to_string())
                .join("audit"),
            "push.result",
            Details {
                account_id: Some(account.id),
                message_ids: &[item.message_id],
                selection,
                knowledge_id: None,
                value: Some(&value),
            },
        )?;
    }
    let resolved = report
        .items
        .iter()
        .filter(|item| {
            matches!(
                item.outcome,
                ItemOutcome::Mutated | ItemOutcome::AlreadySatisfied | ItemOutcome::Missing
            )
        })
        .flat_map(|item| item.context_ids.iter().copied())
        .collect::<HashSet<_>>();
    crate::recovery::gc_after_push_unlocked(repository, account, &resolved)?;
    if report.outcome == PushOutcome::Success {
        report.last_successful_push_ms =
            Some(crate::pull::record_successful_push(repository, account.id)?);
    }
    crate::integrity::commit_push_scope(
        repository,
        account.id,
        &affected.into_iter().collect::<Vec<_>>(),
    )?;
    Ok(report)
}

fn scoped_items(
    repository: &Repository,
    account: &AccountConfig,
    scope: &PushScope,
) -> Result<Vec<triage::StagedItem>> {
    let mut staged = triage::staged_items(repository, account.id)?;
    match scope {
        PushScope::AllStaged => {}
        PushScope::Message(id) => {
            staged.retain(|item| item.message_id == *id);
            if staged.is_empty() {
                return Err(io::Error::other(format!("work item is not staged: {id}")).into());
            }
        }
        PushScope::Selection(name) => {
            let selected: HashSet<_> = triage::selection_ids(repository, account.id, name)?
                .into_iter()
                .collect();
            staged.retain(|item| selected.contains(&item.message_id));
        }
    }
    staged.sort_by_key(|item| item.message_id);
    Ok(staged)
}

fn execute(provider: &dyn MailProvider, items: &mut [PushItem]) {
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let results = Mutex::new(Vec::new());
    let workers = items.len().min(MAX_WORKERS);
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    if stop.load(Ordering::Acquire) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(index) else { break };
                    let result = execute_item(provider, item);
                    if result.1 == Some("authentication") {
                        stop.store(true, Ordering::Release);
                    }
                    results
                        .lock()
                        .expect("push result lock")
                        .push((index, result));
                }
            });
        }
    });
    for (index, (outcome, failure_kind)) in results.into_inner().expect("push results") {
        items[index].outcome = outcome;
        items[index].failure_kind = failure_kind;
    }
    for item in items {
        if item.outcome == ItemOutcome::Planned {
            item.outcome = ItemOutcome::NotAttempted;
        }
    }
}

fn execute_item(
    provider: &dyn MailProvider,
    item: &PushItem,
) -> (ItemOutcome, Option<&'static str>) {
    let state = match provider.push_state(&item.provider_id) {
        Ok(Some(state)) => state,
        Ok(None) => return (ItemOutcome::Missing, None),
        Err(error) => return provider_failure(error),
    };
    match item.action {
        PushAction::Read if !state.unread => (ItemOutcome::AlreadySatisfied, None),
        PushAction::Delete if state.trash => (ItemOutcome::AlreadySatisfied, None),
        PushAction::Read => verify_mutation(provider.mark_read(&item.provider_id), false),
        PushAction::Delete => verify_mutation(provider.trash(&item.provider_id), true),
    }
}

fn verify_mutation(
    result: Result<PushMessageState>,
    expect_trash: bool,
) -> (ItemOutcome, Option<&'static str>) {
    match result {
        Ok(state)
            if if expect_trash {
                state.trash
            } else {
                !state.unread
            } =>
        {
            (ItemOutcome::Mutated, None)
        }
        Ok(_) => (ItemOutcome::Failed, Some("verification")),
        Err(error) => provider_failure(error),
    }
}

fn provider_failure(
    error: Box<dyn std::error::Error + Send + Sync>,
) -> (ItemOutcome, Option<&'static str>) {
    match error.downcast_ref::<ProviderError>().map(|error| error.0) {
        Some(ProviderErrorKind::Missing) => (ItemOutcome::Missing, None),
        Some(ProviderErrorKind::Authentication) => (ItemOutcome::Failed, Some("authentication")),
        _ => (ItemOutcome::Failed, Some("permanent")),
    }
}

fn outcome_label(outcome: ItemOutcome) -> &'static str {
    match outcome {
        ItemOutcome::Planned => "planned",
        ItemOutcome::Mutated => "mutated",
        ItemOutcome::AlreadySatisfied => "already_satisfied",
        ItemOutcome::Missing => "missing",
        ItemOutcome::Failed => "failed",
        ItemOutcome::NotAttempted => "not_attempted",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, fs, sync::atomic::AtomicUsize};

    use crate::{
        provider::{HistoryPage, MessageRef, MessageState, Page},
        repository::{GitIgnorePolicy, NewAccount},
        storage::{MailboxFlags, MessageInput, MimePartInput, ThreadInput, TransferEncoding},
    };

    struct Fake {
        states: Mutex<std::collections::HashMap<String, Option<PushMessageState>>>,
        fail: Option<String>,
        auth: Option<String>,
    }

    impl MailProvider for Fake {
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
            unreachable!()
        }
        fn raw(&self, _: &str) -> Result<Vec<u8>> {
            unreachable!()
        }
        fn push_state(&self, id: &str) -> Result<Option<PushMessageState>> {
            if self.auth.as_deref() == Some(id) {
                return Err(ProviderError(ProviderErrorKind::Authentication, "reauthorize").into());
            }
            if self.fail.as_deref() == Some(id) {
                return Err(ProviderError(ProviderErrorKind::Permanent, "permanent").into());
            }
            Ok(self.states.lock().unwrap().get(id).cloned().flatten())
        }
        fn mark_read(&self, id: &str) -> Result<PushMessageState> {
            let mut states = self.states.lock().unwrap();
            let state = states.get_mut(id).and_then(Option::as_mut).unwrap();
            state.unread = false;
            Ok(state.clone())
        }
        fn trash(&self, id: &str) -> Result<PushMessageState> {
            let mut states = self.states.lock().unwrap();
            let state = states.get_mut(id).and_then(Option::as_mut).unwrap();
            state.trash = true;
            Ok(state.clone())
        }
    }

    fn message(id: &str) -> MessageInput {
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

    fn fixture() -> (tempfile::TempDir, Repository, AccountConfig, Vec<Uuid>) {
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
        let ids = CanonicalStore::new(&repository, &account)
            .unwrap()
            .materialize_thread(&ThreadInput {
                provider: "gmail".into(),
                provider_thread_id: "thread".into(),
                messages: vec![message("a"), message("b")],
            })
            .unwrap();
        for id in &ids {
            triage::write_pending(&repository, account.id, *id).unwrap();
        }
        (directory, repository, account, ids)
    }

    fn state(unread: bool, trash: bool) -> PushMessageState {
        PushMessageState { unread, trash }
    }

    #[test]
    fn idempotent_read_and_trash_require_two_reviews_and_resolve_locally() {
        let (_directory, repository, account, ids) = fixture();
        triage::stage(&repository, &account, &[ids[0]], WorkState::Read).unwrap();
        triage::stage(&repository, &account, &[ids[1]], WorkState::Delete).unwrap();
        let reviews = AtomicUsize::new(0);
        let report = push_account(
            &repository,
            &account,
            PushOptions {
                scope: PushScope::AllStaged,
                dry_run: false,
            },
            || {
                Ok(Box::new(Fake {
                    states: Mutex::new(std::collections::HashMap::from([
                        ("a".into(), Some(state(false, false))),
                        ("b".into(), Some(state(false, true))),
                    ])),
                    fail: None,
                    auth: None,
                }))
            },
            |_, _| {
                reviews.fetch_add(1, Ordering::Relaxed);
                Ok(true)
            },
        )
        .unwrap();
        assert_eq!(
            reviews.load(Ordering::Relaxed),
            2,
            "threaded delete needs extra review"
        );
        assert_eq!(report.outcome, PushOutcome::Success);
        assert!(
            report
                .items
                .iter()
                .all(|item| item.outcome == ItemOutcome::AlreadySatisfied)
        );
        assert!(!triage::staged(&repository, account.id).unwrap());
        assert!(report.last_successful_push_ms.is_some());
        let state = fs::read_to_string(
            repository
                .root()
                .join(".bit-mail/accounts")
                .join(account.id.to_string())
                .join("provider-state.json"),
        )
        .unwrap();
        assert!(state.contains("last_successful_push_ms"));
        assert!(
            !repository
                .data_dir(account.id)
                .join("messages")
                .join(ids[0].to_string())
                .exists(),
            "the now-unreachable thread cache is collected"
        );
    }

    #[test]
    fn missing_resolves_but_permanent_failure_remains_staged() {
        let (_directory, repository, account, ids) = fixture();
        triage::stage(&repository, &account, &ids, WorkState::Read).unwrap();
        triage::create_selection(&repository, &account, "both").unwrap();
        triage::add_selection(&repository, &account, "both", &ids).unwrap();
        let report = push_account(
            &repository,
            &account,
            PushOptions {
                scope: PushScope::AllStaged,
                dry_run: false,
            },
            || {
                Ok(Box::new(Fake {
                    states: Mutex::new(std::collections::HashMap::from([
                        ("a".into(), None),
                        ("b".into(), Some(state(true, false))),
                    ])),
                    fail: Some("b".into()),
                    auth: None,
                }))
            },
            |_, _| Ok(true),
        )
        .unwrap();
        assert_eq!(report.outcome, PushOutcome::PartialFailure);
        assert_eq!(
            report
                .items
                .iter()
                .filter(|item| item.outcome == ItemOutcome::Missing)
                .count(),
            1
        );
        let remaining = triage::staged_items(&repository, account.id).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].message_id, ids[1]);
        assert_eq!(
            triage::show_selection(&repository, &account, "both")
                .unwrap()
                .message_ids,
            [ids[1]],
            "only the successfully resolved member is pruned"
        );
        assert!(report.last_successful_push_ms.is_none());
    }

    #[test]
    fn declining_threaded_delete_confirmation_mutates_nothing() {
        let (_directory, repository, account, ids) = fixture();
        triage::stage(&repository, &account, &[ids[0]], WorkState::Delete).unwrap();
        let provider_calls = AtomicUsize::new(0);
        let reviews = AtomicUsize::new(0);
        let report = push_account(
            &repository,
            &account,
            PushOptions {
                scope: PushScope::AllStaged,
                dry_run: false,
            },
            || {
                provider_calls.fetch_add(1, Ordering::Relaxed);
                unreachable!()
            },
            |stage, _| {
                reviews.fetch_add(1, Ordering::Relaxed);
                Ok(stage == ReviewStage::Normal)
            },
        )
        .unwrap();
        assert_eq!(report.outcome, PushOutcome::Cancelled);
        assert_eq!(reviews.load(Ordering::Relaxed), 2);
        assert_eq!(provider_calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            triage::staged_items(&repository, account.id).unwrap().len(),
            1
        );
    }

    #[test]
    fn authentication_failure_is_terminal_and_remains_staged() {
        let (_directory, repository, account, ids) = fixture();
        triage::stage(&repository, &account, &[ids[0]], WorkState::Read).unwrap();
        let report = push_account(
            &repository,
            &account,
            PushOptions {
                scope: PushScope::Message(ids[0]),
                dry_run: false,
            },
            || {
                Ok(Box::new(Fake {
                    states: Mutex::new(std::collections::HashMap::from([(
                        "a".into(),
                        Some(state(true, false)),
                    )])),
                    fail: None,
                    auth: Some("a".into()),
                }))
            },
            |_, _| Ok(true),
        )
        .unwrap();
        assert_eq!(report.outcome, PushOutcome::AuthenticationFailed);
        assert_eq!(report.items[0].failure_kind, Some("authentication"));
        assert_eq!(
            triage::staged_items(&repository, account.id).unwrap().len(),
            1
        );
        assert!(report.last_successful_push_ms.is_none());
    }

    #[test]
    fn dry_run_and_integrity_failure_never_construct_provider() {
        let (_directory, repository, account, ids) = fixture();
        triage::stage(&repository, &account, &[ids[0]], WorkState::Read).unwrap();
        let calls = AtomicUsize::new(0);
        let report = push_account(
            &repository,
            &account,
            PushOptions {
                scope: PushScope::Message(ids[0]),
                dry_run: true,
            },
            || {
                calls.fetch_add(1, Ordering::Relaxed);
                unreachable!()
            },
            |_, _| unreachable!(),
        )
        .unwrap();
        assert_eq!(report.outcome, PushOutcome::Preview);
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        fs::write(
            repository
                .data_dir(account.id)
                .join("messages")
                .join(ids[0].to_string())
                .join("content.md"),
            "tampered",
        )
        .unwrap();
        assert!(
            push_account(
                &repository,
                &account,
                PushOptions {
                    scope: PushScope::Message(ids[0]),
                    dry_run: false
                },
                || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    unreachable!()
                },
                |_, _| Ok(true),
            )
            .is_err()
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn selection_scope_does_not_broaden_and_json_shape_is_versioned() {
        let (_directory, repository, account, ids) = fixture();
        triage::stage(&repository, &account, &ids, WorkState::Read).unwrap();
        triage::create_selection(&repository, &account, "one").unwrap();
        triage::add_selection(&repository, &account, "one", &[ids[0]]).unwrap();
        let report = push_account(
            &repository,
            &account,
            PushOptions {
                scope: PushScope::Selection("one".into()),
                dry_run: true,
            },
            || unreachable!(),
            |_, _| unreachable!(),
        )
        .unwrap();
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].message_id, ids[0]);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["schema_version"], 1);
        assert_eq!(json["scope"]["kind"], "selection");
        assert_eq!(json["outcome"], "preview");
        assert_eq!(json["items"][0]["outcome"], "planned");
        assert!(json["items"][0].get("provider_id").is_none());
    }

    #[test]
    fn work_item_identity_mismatch_prevents_provider_mutation() {
        let (_directory, repository, account, ids) = fixture();
        triage::stage(&repository, &account, &[ids[0]], WorkState::Read).unwrap();
        let path = repository
            .root()
            .join(".bit-mail/accounts")
            .join(account.id.to_string())
            .join("work-items")
            .join(format!("{}.json", ids[0]));
        let mut item: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        item["message_id"] = ids[1].to_string().into();
        fs::write(path, serde_json::to_vec_pretty(&item).unwrap()).unwrap();
        let calls = AtomicUsize::new(0);
        let error = push_account(
            &repository,
            &account,
            PushOptions {
                scope: PushScope::AllStaged,
                dry_run: false,
            },
            || {
                calls.fetch_add(1, Ordering::Relaxed);
                unreachable!()
            },
            |_, _| Ok(true),
        )
        .unwrap_err();
        assert!(error.to_string().contains("work-item schema or identity"));
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn cleanup_integrity_failure_prevents_provider_mutation() {
        let (_directory, repository, account, ids) = fixture();
        triage::stage(&repository, &account, &[ids[0]], WorkState::Read).unwrap();
        triage::create_selection(&repository, &account, "other").unwrap();
        let selection = repository
            .root()
            .join(".bit-mail/accounts")
            .join(account.id.to_string())
            .join("selections/other.json");
        fs::write(selection, b"{}\n").unwrap();
        let calls = AtomicUsize::new(0);
        assert!(
            push_account(
                &repository,
                &account,
                PushOptions {
                    scope: PushScope::Message(ids[0]),
                    dry_run: false,
                },
                || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    unreachable!()
                },
                |_, _| Ok(true),
            )
            .is_err()
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn scoped_push_preserves_unrelated_cache_and_integrity_mismatch() {
        let (_directory, repository, account, ids) = fixture();
        triage::stage(&repository, &account, &[ids[0]], WorkState::Read).unwrap();
        let mut unrelated = message("c");
        unrelated.provider_thread_id = "other".into();
        let unrelated_id = CanonicalStore::new(&repository, &account)
            .unwrap()
            .materialize_thread(&ThreadInput {
                provider: "gmail".into(),
                provider_thread_id: "other".into(),
                messages: vec![unrelated],
            })
            .unwrap()[0];
        let content = CanonicalStore::new(&repository, &account)
            .unwrap()
            .message_path(unrelated_id)
            .join("content.md");
        fs::write(&content, "tampered unrelated cache").unwrap();
        let report = push_account(
            &repository,
            &account,
            PushOptions {
                scope: PushScope::Message(ids[0]),
                dry_run: false,
            },
            || {
                Ok(Box::new(Fake {
                    states: Mutex::new(std::collections::HashMap::from([(
                        "a".into(),
                        Some(state(true, false)),
                    )])),
                    fail: None,
                    auth: None,
                }))
            },
            |_, _| Ok(true),
        )
        .unwrap();
        assert_eq!(report.outcome, PushOutcome::Success);
        assert!(
            content.is_file(),
            "push GC must not broaden beyond its scope"
        );
        assert!(
            crate::integrity::validate_account(&repository, account.id)
                .unwrap()
                .iter()
                .any(|mismatch| mismatch.path.ends_with("/content.md")),
            "incremental commit must not bless unrelated corruption"
        );
    }
}
