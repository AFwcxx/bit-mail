use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
};

use bit_mail::{
    Result,
    gmail::GmailClient,
    provider::{HistoryPage, MailProvider, MessageRef, MessageState, Page, PushMessageState},
    pull::{Outcome, PullOptions},
    push::{PushOptions, PushOutcome, PushScope},
    repository::{AccountConfig, GitIgnorePolicy, NewAccount, Repository},
    storage::{MailboxFlags, MessageInput, MimePartInput, ThreadInput, TransferEncoding},
    triage::{self, WorkState},
};

#[derive(Default)]
struct ProviderState {
    unread: bool,
    trash: bool,
}

struct LifecycleProvider(Arc<Mutex<ProviderState>>);

fn gmail_server<F>(requests: usize, response: F) -> (String, thread::JoinHandle<()>)
where
    F: Fn(&str) -> (&'static str, String) + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for _ in 0..requests {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            let mut content_length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" {
                    break;
                }
                if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = value.trim().parse().unwrap();
                }
            }
            reader.read_exact(&mut vec![0; content_length]).unwrap();
            let (status, body) = response(&request);
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    (base, server)
}

fn repository() -> (tempfile::TempDir, Repository, AccountConfig) {
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
    (directory, repository, account)
}

fn gmail_thread(id: &str) -> String {
    format!(
        r#"{{"id":"thread-{id}","messages":[{{"id":"{id}","threadId":"thread-{id}","labelIds":["INBOX","UNREAD"],"internalDate":"1000","payload":{{"partId":"0","mimeType":"text/plain","body":{{"size":7,"data":"Zml4dHVyZQ"}}}}}}]}}"#
    )
}

impl LifecycleProvider {
    fn message(id: &str, unread: bool) -> MessageInput {
        MessageInput {
            provider_message_id: id.into(),
            provider_thread_id: "thread".into(),
            received_at_ms: 1,
            sent_at_ms: None,
            subject: Some("controlled fixture".into()),
            from: vec![],
            to: vec![],
            cc: vec![],
            bcc: vec![],
            reply_to: vec![],
            rfc_message_id: None,
            flags: MailboxFlags {
                inbox: unread,
                unread,
                ..Default::default()
            },
            parts: vec![MimePartInput {
                id: "0".into(),
                mime_type: "text/plain".into(),
                headers: BTreeMap::new(),
                filename: None,
                transfer_encoding: TransferEncoding::None,
                body: Some(b"fixture".to_vec()),
                remote: None,
                parts: vec![],
            }],
            provider_source: serde_json::json!({"id": id}),
        }
    }
}

impl MailProvider for LifecycleProvider {
    fn current_history_id(&self) -> Result<String> {
        Ok("10".into())
    }

    fn unread_page(&self, _: Option<&str>, _: u32) -> Result<Page<MessageRef>> {
        Ok(Page {
            items: vec![MessageRef {
                id: "actionable".into(),
                thread_id: "thread".into(),
            }],
            next_page: None,
        })
    }

    fn history_page(&self, _: &str, _: Option<&str>) -> Result<HistoryPage> {
        Ok(HistoryPage {
            changed: vec![],
            next_page: None,
            history_id: "11".into(),
        })
    }

    fn message_state(&self, _: &str) -> Result<MessageState> {
        Ok(MessageState::Actionable)
    }

    fn thread(&self, _: &str) -> Result<ThreadInput> {
        Ok(ThreadInput {
            provider: "gmail".into(),
            provider_thread_id: "thread".into(),
            messages: vec![
                Self::message("context", false),
                Self::message("actionable", true),
            ],
        })
    }

    fn attachment(&self, _: &str, _: &str) -> Result<Vec<u8>> {
        unreachable!()
    }

    fn raw(&self, _: &str) -> Result<Vec<u8>> {
        unreachable!()
    }

    fn push_state(&self, _: &str) -> Result<Option<PushMessageState>> {
        let state = self.0.lock().unwrap();
        Ok(Some(PushMessageState {
            unread: state.unread,
            trash: state.trash,
        }))
    }

    fn mark_read(&self, _: &str) -> Result<PushMessageState> {
        let mut state = self.0.lock().unwrap();
        state.unread = false;
        Ok(PushMessageState {
            unread: state.unread,
            trash: state.trash,
        })
    }

    fn trash(&self, _: &str) -> Result<PushMessageState> {
        unreachable!()
    }
}

#[test]
fn fake_provider_pull_stage_blocked_pull_and_push_lifecycle() {
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
    let provider = Arc::new(Mutex::new(ProviderState {
        unread: true,
        trash: false,
    }));

    let pulled = bit_mail::pull::pull_account(
        &repository,
        &account,
        PullOptions {
            limit: 10,
            all: false,
        },
        || Ok(Box::new(LifecycleProvider(provider.clone()))),
    )
    .unwrap();
    assert!(matches!(pulled.outcome, Outcome::Success));
    let items = triage::work_items(&repository, &account, None).unwrap();
    assert_eq!(
        items.work_items.len(),
        1,
        "context-only mail must not become stageable"
    );
    let actionable = items.work_items[0].message_id;
    assert_eq!(items.work_items[0].context.len(), 2);
    triage::stage(&repository, &account, &[actionable], WorkState::Read).unwrap();

    let blocked = bit_mail::pull::pull_account(
        &repository,
        &account,
        PullOptions {
            limit: 10,
            all: false,
        },
        || panic!("staged intent must block before provider construction"),
    )
    .unwrap();
    assert!(matches!(blocked.outcome, Outcome::Blocked));

    let pushed = bit_mail::push::push_account(
        &repository,
        &account,
        PushOptions {
            scope: PushScope::Message(actionable),
            dry_run: false,
        },
        || Ok(Box::new(LifecycleProvider(provider.clone()))),
        |_, _| Ok(true),
    )
    .unwrap();
    assert_eq!(pushed.outcome, PushOutcome::Success);
    assert!(!provider.lock().unwrap().unread);
    assert!(
        triage::work_items(&repository, &account, None)
            .unwrap()
            .work_items
            .is_empty()
    );
    let status = bit_mail::status::collect(&repository, vec![account]).unwrap();
    assert_eq!(
        (status[0].pending, status[0].read, status[0].delete),
        (0, 0, 0)
    );
    assert!(status[0].last_successful_pull_ms.is_some());
    assert!(status[0].last_successful_push_ms.is_some());
    assert!(
        bit_mail::integrity::validate_full(&repository)
            .unwrap()
            .mismatches
            .is_empty()
    );
}

#[test]
fn gmail_mock_partial_pull_publishes_only_complete_threads() {
    let (_directory, repository, account) = repository();
    let (base, server) = gmail_server(4, |request| {
        if request.starts_with("GET /gmail/v1/users/me/profile?") {
            (
                "200 OK",
                r#"{"emailAddress":"test@example.com","historyId":"9"}"#.into(),
            )
        } else if request.starts_with("GET /gmail/v1/users/me/messages?") {
            (
                "200 OK",
                r#"{"messages":[{"id":"good","threadId":"thread-good"},{"id":"bad","threadId":"thread-bad"}]}"#.into(),
            )
        } else if request.starts_with("GET /gmail/v1/users/me/threads/thread-good?") {
            ("200 OK", gmail_thread("good"))
        } else if request.starts_with("GET /gmail/v1/users/me/threads/thread-bad?") {
            ("404 Not Found", String::new())
        } else {
            panic!("unexpected Gmail request: {request}")
        }
    });
    let report = bit_mail::pull::pull_account(
        &repository,
        &account,
        PullOptions {
            limit: 2,
            all: false,
        },
        move || Ok(Box::new(GmailClient::new("token", base)?)),
    )
    .unwrap();

    assert!(matches!(report.outcome, Outcome::Failed));
    assert_eq!(
        (report.threads, report.failures, report.new_work_items),
        (2, 1, 1)
    );
    let items = triage::work_items(&repository, &account, None).unwrap();
    assert_eq!(items.work_items.len(), 1);
    assert_eq!(
        std::fs::read_dir(repository.data_dir(account.id).join("messages"))
            .unwrap()
            .count(),
        1,
        "the failed thread must not publish canonical data"
    );
    assert!(
        bit_mail::pull::provider_status(&repository, account.id)
            .unwrap()
            .last_successful_pull_ms
            .is_none()
    );
    server.join().unwrap();
}

#[test]
fn gmail_mock_partial_push_resolves_only_successful_messages() {
    let (_directory, repository, account) = repository();
    let (base, server) = gmail_server(8, |request| {
        if request.starts_with("GET /gmail/v1/users/me/profile?") {
            (
                "200 OK",
                r#"{"emailAddress":"test@example.com","historyId":"9"}"#.into(),
            )
        } else if request.starts_with("GET /gmail/v1/users/me/messages?") {
            (
                "200 OK",
                r#"{"messages":[{"id":"a","threadId":"thread-a"},{"id":"b","threadId":"thread-b"}]}"#.into(),
            )
        } else if request.starts_with("GET /gmail/v1/users/me/threads/thread-a?") {
            ("200 OK", gmail_thread("a"))
        } else if request.starts_with("GET /gmail/v1/users/me/threads/thread-b?") {
            ("200 OK", gmail_thread("b"))
        } else if request.starts_with("GET /gmail/v1/users/me/messages/a?") {
            (
                "200 OK",
                r#"{"id":"a","threadId":"thread-a","labelIds":["INBOX","UNREAD"]}"#.into(),
            )
        } else if request.starts_with("GET /gmail/v1/users/me/messages/b?") {
            (
                "200 OK",
                r#"{"id":"b","threadId":"thread-b","labelIds":["INBOX","UNREAD"]}"#.into(),
            )
        } else if request.starts_with("POST /gmail/v1/users/me/messages/a/modify ") {
            (
                "200 OK",
                r#"{"id":"a","threadId":"thread-a","labelIds":[]}"#.into(),
            )
        } else if request.starts_with("POST /gmail/v1/users/me/messages/b/modify ") {
            ("400 Bad Request", String::new())
        } else {
            panic!("unexpected Gmail request: {request}")
        }
    });
    bit_mail::pull::pull_account(
        &repository,
        &account,
        PullOptions {
            limit: 2,
            all: false,
        },
        {
            let base = base.clone();
            move || Ok(Box::new(GmailClient::new("token", base)?))
        },
    )
    .unwrap();
    let ids = triage::work_items(&repository, &account, None)
        .unwrap()
        .work_items
        .into_iter()
        .map(|item| item.message_id)
        .collect::<Vec<_>>();
    triage::stage(&repository, &account, &ids, WorkState::Read).unwrap();
    let report = bit_mail::push::push_account(
        &repository,
        &account,
        PushOptions {
            scope: PushScope::AllStaged,
            dry_run: false,
        },
        move || Ok(Box::new(GmailClient::new("token", base)?)),
        |_, _| Ok(true),
    )
    .unwrap();

    assert_eq!(report.outcome, PushOutcome::PartialFailure);
    let remaining = triage::work_items(&repository, &account, Some(WorkState::Read)).unwrap();
    assert_eq!(remaining.work_items.len(), 1);
    let failed = report
        .items
        .iter()
        .find(|item| item.outcome == bit_mail::push::ItemOutcome::Failed)
        .unwrap();
    assert_eq!(
        remaining.work_items[0].message_id, failed.message_id,
        "the failed Gmail mutation must remain staged"
    );
    assert!(report.last_successful_push_ms.is_none());
    server.join().unwrap();
}
