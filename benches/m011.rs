use std::{
    collections::BTreeMap, env, fs, fs::File, io::Read, path::PathBuf, process::Command,
    time::Duration,
};

use bit_mail::{
    Result,
    credentials::{CredentialId, CredentialStore},
    provider::{
        HistoryPage, MailProvider, MessageRef, MessageState, Page, ProviderError,
        ProviderErrorKind, PushMessageState,
    },
    pull::{PullOptions, pull_account},
    push::{PushOptions, PushScope, push_account},
    repository::{AccountConfig, GitIgnorePolicy, NewAccount, Repository},
    storage::{
        CanonicalStore, MailboxFlags, MessageInput, MimePartInput, ThreadInput, TransferEncoding,
    },
    triage::{self, WorkState},
};
use criterion::{BatchSize, Criterion, SamplingMode, Throughput, criterion_group, criterion_main};
use uuid::Uuid;

struct Fixture {
    _directory: tempfile::TempDir,
    repository: Repository,
    account: AccountConfig,
}

impl Fixture {
    fn status_items(count: usize) -> Self {
        let fixture = Self::empty();
        let work = account_dir(&fixture).join("work-items");
        fs::create_dir_all(&work).unwrap();
        for index in 0..count {
            let id = fixture_id(index);
            let state = ["pending", "read", "delete"][index % 3];
            write_json(
                work.join(format!("{id}.json")),
                &serde_json::json!({"schema_version": 1, "message_id": id, "state": state}),
            );
        }
        bit_mail::integrity::rebuild_full(&fixture.repository).unwrap();
        fixture
    }

    fn canonical_items(count: usize) -> Self {
        let fixture = Self::empty();
        let account = account_dir(&fixture);
        let identities = account.join("identities/messages");
        let provider = account.join("provider/messages");
        let threads = account.join("threads");
        let work = account.join("work-items");
        let messages = fixture
            .repository
            .data_dir(fixture.account.id)
            .join("messages");
        for path in [&identities, &provider, &threads, &work, &messages] {
            fs::create_dir_all(path).unwrap();
        }
        for index in 0..count {
            let id = fixture_id(index);
            let provider_id = format!("provider-{index}");
            write_json(
                identities.join(format!("{id}.json")),
                &serde_json::json!({
                    "schema_version": 1, "provider": "gmail",
                    "provider_message_id": provider_id, "message_id": id
                }),
            );
            write_json(
                provider.join(format!("{id}.json")),
                &serde_json::json!({
                    "schema_version": 1, "provider": "gmail",
                    "provider_message_id": provider_id,
                    "provider_thread_id": format!("thread-{index}"),
                    "source": {"fixture": true}, "remote_attachments": []
                }),
            );
            write_json(
                threads.join(format!("{id}.json")),
                &serde_json::json!({
                    "schema_version": 1, "provider": "gmail",
                    "provider_thread_id": format!("thread-{index}"), "messages": [id]
                }),
            );
            write_json(
                work.join(format!("{id}.json")),
                &serde_json::json!({"schema_version": 1, "message_id": id, "state": "pending"}),
            );
            let directory = messages.join(id.to_string());
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("content.md"), "small fixture\n").unwrap();
            write_json(
                directory.join("metadata.json"),
                &serde_json::json!({
                    "schema_version": 1, "id": id, "received_at_ms": index,
                    "from": [], "to": [], "cc": [], "bcc": [], "reply_to": [],
                    "flags": {"inbox": true, "unread": true, "sent": false, "trash": false},
                    "attachments": [], "normalization": "complete"
                }),
            );
        }
        bit_mail::integrity::rebuild_full(&fixture.repository).unwrap();
        CanonicalStore::new(&fixture.repository, &fixture.account)
            .unwrap()
            .rebuild_index()
            .unwrap();
        assert_eq!(
            triage::work_items(&fixture.repository, &fixture.account, None)
                .unwrap()
                .work_items
                .len(),
            count
        );
        fixture
    }

    fn empty() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let account = repository
            .create_account(NewAccount {
                alias: "benchmark",
                provider: "gmail",
                provider_identity: None,
                credential_profile: None,
            })
            .unwrap();
        Self {
            _directory: directory,
            repository,
            account,
        }
    }
}

fn account_dir(fixture: &Fixture) -> PathBuf {
    fixture
        .repository
        .root()
        .join(".bit-mail/accounts")
        .join(fixture.account.id.to_string())
}

fn fixture_id(index: usize) -> Uuid {
    let value = index as u128 + 1;
    Uuid::from_u128((value << 64) | value)
}

fn write_json(path: PathBuf, value: &serde_json::Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}

fn env_size(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

struct EmptyCredentials;
impl CredentialStore for EmptyCredentials {
    fn get(&self, _: CredentialId) -> Result<Option<String>> {
        Ok(None)
    }
    fn set(&self, _: CredentialId, _: &str) -> Result<()> {
        unreachable!()
    }
    fn delete(&self, _: CredentialId) -> Result<()> {
        unreachable!()
    }
}

fn repository_benchmarks(c: &mut Criterion) {
    let status_count = env_size("BIT_MAIL_BENCH_ITEMS", 10_000);
    let status = Fixture::status_items(status_count);
    let nested = status.repository.root().join("nested/path");
    fs::create_dir_all(&nested).unwrap();
    let mut group = c.benchmark_group("repository_structural");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(3));
    group.bench_function("process_startup", |b| {
        b.iter(|| {
            let output = Command::new(env!("CARGO_BIN_EXE_bit-mail"))
                .arg("--version")
                .output()
                .unwrap();
            assert!(output.status.success());
            std::hint::black_box(output)
        })
    });
    group.bench_function("discovery", |b| {
        b.iter(|| Repository::discover_from(std::hint::black_box(&nested)).unwrap())
    });
    group.throughput(Throughput::Elements(status_count as u64));
    group.bench_function("status", |b| {
        b.iter(|| {
            bit_mail::status::collect(
                &status.repository,
                std::hint::black_box(vec![status.account.clone()]),
            )
            .unwrap()
        })
    });
    group.finish();

    let canonical_count = env_size("BIT_MAIL_BENCH_CANONICAL", 1_000);
    let canonical = Fixture::canonical_items(canonical_count);
    let store = CanonicalStore::new(&canonical.repository, &canonical.account).unwrap();
    let mut group = c.benchmark_group("canonical_structural");
    group
        .sample_size(10)
        .measurement_time(Duration::from_secs(3));
    group.throughput(Throughput::Elements(canonical_count as u64));
    group.bench_function("work_items", |b| {
        b.iter(|| triage::work_items(&canonical.repository, &canonical.account, None).unwrap())
    });
    group.bench_function("sqlite_rebuild", |b| {
        b.iter(|| store.rebuild_index().unwrap())
    });
    group.bench_function("merkle_verify", |b| {
        b.iter(|| {
            bit_mail::integrity::validate_account(&canonical.repository, canonical.account.id)
                .unwrap()
        })
    });
    group.bench_function("doctor_full", |b| {
        b.iter(|| {
            bit_mail::diagnostics::run(
                &canonical.repository,
                bit_mail::diagnostics::Options {
                    account: Some(&canonical.account.alias),
                    all_accounts: false,
                    full: true,
                    online: false,
                },
                &EmptyCredentials,
                |_| Ok(()),
            )
        })
    });
    group.finish();
}

fn hashing_benchmarks(c: &mut Criterion) {
    let bytes = env_size("BIT_MAIL_BENCH_ATTACHMENT_BYTES", 64 * 1024 * 1024);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("attachment.bin");
    fs::write(&path, vec![0x5a; bytes]).unwrap();
    let mmap = unsafe { memmap2::Mmap::map(&File::open(&path).unwrap()).unwrap() };
    let mut group = c.benchmark_group("large_attachment_hashing");
    group.sample_size(10).sampling_mode(SamplingMode::Flat);
    group.throughput(Throughput::Bytes(bytes as u64));
    group.bench_function("buffered", |b| {
        b.iter(|| {
            let mut file = File::open(&path).unwrap();
            let mut hasher = blake3::Hasher::new();
            let mut buffer = [0_u8; 128 * 1024];
            loop {
                let read = file.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            hasher.finalize()
        })
    });
    group.bench_function("mmap_parallel", |b| {
        b.iter(|| {
            let mut hasher = blake3::Hasher::new();
            hasher.update_rayon(&mmap);
            hasher.finalize()
        })
    });
    group.finish();
}

fn message(id: &str, thread: &str, body_size: usize) -> MessageInput {
    MessageInput {
        provider_message_id: id.into(),
        provider_thread_id: thread.into(),
        received_at_ms: 1,
        sent_at_ms: None,
        subject: Some("benchmark".into()),
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
            body: Some(vec![b'x'; body_size]),
            remote: None,
            parts: vec![],
        }],
        provider_source: serde_json::json!({"fixture": true}),
    }
}

fn materialization_benchmark(c: &mut Criterion) {
    let fixture = Fixture::empty();
    let store = CanonicalStore::new(&fixture.repository, &fixture.account).unwrap();
    let count = env_size("BIT_MAIL_BENCH_THREAD_MESSAGES", 100);
    let thread = ThreadInput {
        provider: "gmail".into(),
        provider_thread_id: "large-thread".into(),
        messages: (0..count)
            .map(|index| message(&format!("message-{index}"), "large-thread", 16 * 1024))
            .collect(),
    };
    let mut group = c.benchmark_group("gmail_like_thread");
    group
        .sample_size(10)
        .throughput(Throughput::Elements(count as u64));
    group.bench_function("materialize_normalize", |b| {
        b.iter(|| {
            store
                .materialize_thread(std::hint::black_box(&thread))
                .unwrap()
        })
    });
    group.finish();
}

fn merkle_update_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("merkle_branch_update");
    group.sample_size(10);
    group.bench_function("stage_one_work_item", |b| {
        b.iter_batched(
            || Fixture::canonical_items(1),
            |fixture| {
                triage::stage(
                    &fixture.repository,
                    &fixture.account,
                    &[fixture_id(0)],
                    WorkState::Read,
                )
                .unwrap()
            },
            BatchSize::LargeInput,
        )
    });
    group.finish();
}

struct BenchProvider {
    threads: usize,
    delay: Duration,
}

impl MailProvider for BenchProvider {
    fn current_history_id(&self) -> Result<String> {
        Ok("10".into())
    }
    fn unread_page(&self, _: Option<&str>, _: u32) -> Result<Page<MessageRef>> {
        Ok(Page {
            items: (0..self.threads)
                .map(|index| MessageRef {
                    id: format!("message-{index}"),
                    thread_id: format!("thread-{index}"),
                })
                .collect(),
            next_page: None,
        })
    }
    fn history_page(&self, _: &str, _: Option<&str>) -> Result<HistoryPage> {
        Ok(HistoryPage {
            changed: (0..self.threads)
                .map(|index| MessageRef {
                    id: format!("message-{index}"),
                    thread_id: format!("thread-{index}"),
                })
                .collect(),
            next_page: None,
            history_id: "11".into(),
        })
    }
    fn message_state(&self, _: &str) -> Result<MessageState> {
        Ok(MessageState::Actionable)
    }
    fn thread(&self, id: &str) -> Result<ThreadInput> {
        std::thread::sleep(self.delay);
        let index = id.trim_start_matches("thread-");
        Ok(ThreadInput {
            provider: "gmail".into(),
            provider_thread_id: id.into(),
            messages: vec![message(&format!("message-{index}"), id, 128)],
        })
    }
    fn attachment(&self, _: &str, _: &str) -> Result<Vec<u8>> {
        unreachable!()
    }
    fn raw(&self, _: &str) -> Result<Vec<u8>> {
        unreachable!()
    }
    fn push_state(&self, _: &str) -> Result<Option<PushMessageState>> {
        std::thread::sleep(self.delay);
        Ok(Some(PushMessageState {
            unread: true,
            trash: false,
        }))
    }
    fn mark_read(&self, _: &str) -> Result<PushMessageState> {
        Err(ProviderError(ProviderErrorKind::Permanent, "benchmark failure").into())
    }
}

fn pulled_fixture(threads: usize) -> Fixture {
    let fixture = Fixture::empty();
    pull_account(
        &fixture.repository,
        &fixture.account,
        PullOptions {
            limit: threads as u32,
            all: true,
        },
        || {
            Ok(Box::new(BenchProvider {
                threads,
                delay: Duration::ZERO,
            }))
        },
    )
    .unwrap();
    fixture
}

fn provider_benchmarks(c: &mut Criterion) {
    let threads = 8;
    let pull = pulled_fixture(threads);
    let mut group = c.benchmark_group("bounded_provider_concurrency");
    group
        .sample_size(10)
        .throughput(Throughput::Elements(threads as u64));
    group.bench_function("pull", |b| {
        b.iter(|| {
            pull_account(
                &pull.repository,
                &pull.account,
                PullOptions {
                    limit: threads as u32,
                    all: true,
                },
                || {
                    Ok(Box::new(BenchProvider {
                        threads,
                        delay: Duration::from_millis(2),
                    }))
                },
            )
            .unwrap()
        })
    });
    group.bench_function("push", |b| {
        b.iter_batched(
            || {
                let fixture = pulled_fixture(threads);
                let ids = triage::work_items(&fixture.repository, &fixture.account, None)
                    .unwrap()
                    .work_items
                    .into_iter()
                    .map(|item| item.message_id)
                    .collect::<Vec<_>>();
                triage::stage(&fixture.repository, &fixture.account, &ids, WorkState::Read)
                    .unwrap();
                fixture
            },
            |fixture| {
                push_account(
                    &fixture.repository,
                    &fixture.account,
                    PushOptions {
                        scope: PushScope::AllStaged,
                        dry_run: false,
                    },
                    || {
                        Ok(Box::new(BenchProvider {
                            threads,
                            delay: Duration::from_millis(2),
                        }))
                    },
                    |_, _| Ok(true),
                )
                .unwrap()
            },
            BatchSize::LargeInput,
        )
    });
    group.finish();
}

criterion_group!(
    benches,
    repository_benchmarks,
    hashing_benchmarks,
    materialization_benchmark,
    merkle_update_benchmark,
    provider_benchmarks
);
criterion_main!(benches);
