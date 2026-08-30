use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    Result,
    repository::{AccountConfig, MutationLock, Repository},
};

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Address {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub address: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MailboxFlags {
    pub inbox: bool,
    pub unread: bool,
    pub sent: bool,
    pub trash: bool,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferEncoding {
    #[default]
    None,
    Base64,
    QuotedPrintable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RemoteAttachment {
    pub provider_attachment_id: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentState {
    Local { path: PathBuf },
    Remote(RemoteAttachment),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MimePartInput {
    pub id: String,
    pub mime_type: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default)]
    pub transfer_encoding: TransferEncoding,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteAttachment>,
    #[serde(default)]
    pub parts: Vec<MimePartInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MessageInput {
    pub provider_message_id: String,
    pub provider_thread_id: String,
    pub received_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default)]
    pub from: Vec<Address>,
    #[serde(default)]
    pub to: Vec<Address>,
    #[serde(default)]
    pub cc: Vec<Address>,
    #[serde(default)]
    pub bcc: Vec<Address>,
    #[serde(default)]
    pub reply_to: Vec<Address>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rfc_message_id: Option<String>,
    pub flags: MailboxFlags,
    pub parts: Vec<MimePartInput>,
    pub provider_source: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThreadInput {
    pub provider: String,
    pub provider_thread_id: String,
    pub messages: Vec<MessageInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NormalizationStatus {
    Complete,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AttachmentMetadata {
    pub id: String,
    pub filename: String,
    pub media_type: String,
    pub size: u64,
    pub local: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanonicalMetadata {
    pub schema_version: u32,
    pub id: Uuid,
    pub received_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub from: Vec<Address>,
    pub to: Vec<Address>,
    pub cc: Vec<Address>,
    pub bcc: Vec<Address>,
    pub reply_to: Vec<Address>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rfc_message_id: Option<String>,
    pub flags: MailboxFlags,
    pub attachments: Vec<AttachmentMetadata>,
    pub normalization: NormalizationStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityRecord {
    schema_version: u32,
    provider: String,
    provider_message_id: String,
    message_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThreadManifest {
    schema_version: u32,
    provider: String,
    provider_thread_id: String,
    messages: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderRecord {
    schema_version: u32,
    provider: String,
    provider_message_id: String,
    provider_thread_id: String,
    source: Value,
    remote_attachments: Vec<ProviderAttachmentReference>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderAttachmentReference {
    part_id: String,
    provider_attachment_id: String,
    size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct Diagnostics {
    schema_version: u32,
    message_id: Uuid,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(deny_unknown_fields)]
struct Diagnostic {
    severity: &'static str,
    stage: &'static str,
    code: &'static str,
    detail: String,
}

struct Normalized {
    content: String,
    attachments: Vec<(AttachmentMetadata, Option<Vec<u8>>)>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Clone)]
enum Body {
    Plain(String),
    Html(String),
}

pub struct CanonicalStore {
    root: PathBuf,
    account_id: Uuid,
    provider: String,
}

impl CanonicalStore {
    pub fn new(repository: &Repository, account: &AccountConfig) -> Result<Self> {
        if repository
            .accounts()?
            .iter()
            .all(|configured| configured.id != account.id)
        {
            return Err(error("account is not configured in this repository"));
        }
        Ok(Self {
            root: repository.root().to_path_buf(),
            account_id: account.id,
            provider: account.provider.clone(),
        })
    }

    pub fn materialize_thread(&self, thread: &ThreadInput) -> Result<Vec<Uuid>> {
        let _lock = self.account_lock()?;
        let repository = Repository::open(self.root.clone())?;
        crate::integrity::prepare_account(&repository, self.account_id)?;
        let ids = self.materialize_thread_unlocked(thread)?;
        crate::integrity::commit_account(&repository, self.account_id)?;
        Ok(ids)
    }

    pub(crate) fn materialize_thread_unlocked(&self, thread: &ThreadInput) -> Result<Vec<Uuid>> {
        self.materialize_thread_with_cache(thread, true)
    }

    pub(crate) fn replace_thread_unlocked(&self, thread: &ThreadInput) -> Result<Vec<Uuid>> {
        self.materialize_thread_with_cache(thread, false)
    }

    fn materialize_thread_with_cache(
        &self,
        thread: &ThreadInput,
        preserve_cached_attachments: bool,
    ) -> Result<Vec<Uuid>> {
        if thread.provider != self.provider {
            return Err(error("thread provider does not match account provider"));
        }
        validate_thread(thread)?;
        let mut normalized = thread
            .messages
            .iter()
            .map(|message| normalize_parts(&message.parts))
            .collect::<Result<Vec<_>>>()?;
        self.create_layout()?;

        let mut ids = Vec::with_capacity(thread.messages.len());
        for message in &thread.messages {
            ids.push(self.message_id(&thread.provider, &message.provider_message_id)?);
        }

        for (id, normalized) in ids.iter().zip(&mut normalized) {
            if !preserve_cached_attachments {
                continue;
            }
            let existing_dir = self.data_dir().join(id.to_string());
            let metadata_path = existing_dir.join("metadata.json");
            if !metadata_path.exists() {
                continue;
            }
            let existing: CanonicalMetadata = read_json(&metadata_path)?;
            require_version(existing.schema_version, "message metadata")?;
            for (metadata, bytes) in &mut normalized.attachments {
                if bytes.is_some() {
                    continue;
                }
                let Some(previous) = existing
                    .attachments
                    .iter()
                    .find(|attachment| attachment.id == metadata.id && attachment.local)
                else {
                    continue;
                };
                if previous.size != metadata.size {
                    continue;
                }
                let previous_relative = attachment_relative_path(&previous.id, &previous.filename);
                if previous.relative_path.as_deref() != Some(&previous_relative) {
                    return Err(error(
                        "local attachment path does not match canonical mapping",
                    ));
                }
                let cached = fs::read(existing_dir.join(previous_relative))?;
                if cached.len() as u64 != previous.size {
                    return Err(error("local attachment size does not match metadata"));
                }
                metadata.local = true;
                metadata.relative_path =
                    Some(attachment_relative_path(&metadata.id, &metadata.filename));
                *bytes = Some(cached);
            }
        }

        let data_root = self.data_dir();
        let staging = self
            .staging_dir()
            .join(format!("thread-{}", Uuid::new_v4()));
        create_private_dir(&staging)?;
        let result = (|| -> Result<()> {
            for ((message, id), normalized) in thread.messages.iter().zip(&ids).zip(normalized) {
                let message_dir = staging.join(id.to_string());
                create_private_dir(&message_dir)?;
                write_private(
                    &message_dir.join("content.md"),
                    normalized.content.as_bytes(),
                )?;
                let attachments: Vec<_> = normalized
                    .attachments
                    .iter()
                    .map(|(metadata, _)| metadata.clone())
                    .collect();
                write_json(
                    &message_dir.join("metadata.json"),
                    &CanonicalMetadata {
                        schema_version: SCHEMA_VERSION,
                        id: *id,
                        received_at_ms: message.received_at_ms,
                        sent_at_ms: message.sent_at_ms,
                        subject: message.subject.clone(),
                        from: message.from.clone(),
                        to: message.to.clone(),
                        cc: message.cc.clone(),
                        bcc: message.bcc.clone(),
                        reply_to: message.reply_to.clone(),
                        rfc_message_id: message.rfc_message_id.clone(),
                        flags: message.flags,
                        attachments,
                        normalization: if normalized.diagnostics.is_empty() {
                            NormalizationStatus::Complete
                        } else {
                            NormalizationStatus::Partial
                        },
                    },
                )?;
                for (metadata, bytes) in &normalized.attachments {
                    if let (Some(path), Some(bytes)) = (&metadata.relative_path, bytes) {
                        let path = message_dir.join(path);
                        create_private_dir(path.parent().expect("attachment parent"))?;
                        write_private(&path, bytes)?;
                    }
                }
                write_json_atomic(
                    &self.provider_dir().join(format!("{id}.json")),
                    &ProviderRecord {
                        schema_version: SCHEMA_VERSION,
                        provider: thread.provider.clone(),
                        provider_message_id: message.provider_message_id.clone(),
                        provider_thread_id: message.provider_thread_id.clone(),
                        source: message.provider_source.clone(),
                        remote_attachments: remote_attachment_references(&message.parts),
                    },
                )?;
                let diagnostics_path = self.diagnostics_dir().join(format!("{id}.json"));
                if normalized.diagnostics.is_empty() {
                    remove_if_exists(&diagnostics_path)?;
                } else {
                    write_json_atomic(
                        &diagnostics_path,
                        &Diagnostics {
                            schema_version: SCHEMA_VERSION,
                            message_id: *id,
                            diagnostics: normalized.diagnostics,
                        },
                    )?;
                }
            }

            for id in &ids {
                replace_dir(
                    &staging.join(id.to_string()),
                    &data_root.join(id.to_string()),
                )?;
            }
            self.write_thread_manifest(ThreadManifest {
                schema_version: SCHEMA_VERSION,
                provider: thread.provider.clone(),
                provider_thread_id: thread.provider_thread_id.clone(),
                messages: ids.clone(),
            })?;
            self.rebuild_index_unlocked()
        })();
        if let Err(cleanup) = fs::remove_dir_all(&staging)
            && cleanup.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("warning: failed to remove storage staging directory: {cleanup}");
        }
        result?;
        Ok(ids)
    }

    pub fn rebuild_index(&self) -> Result<()> {
        let _lock = self.account_lock()?;
        self.rebuild_index_unlocked()
    }

    pub(crate) fn rebuild_index_unlocked(&self) -> Result<()> {
        self.create_layout()?;
        let target = self.account_dir().join("index.sqlite");
        let temporary = self
            .account_dir()
            .join(format!("index.sqlite.tmp-{}", Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut connection = Connection::open(&temporary)?;
            connection.execute_batch(
                "PRAGMA user_version = 1;
                 CREATE TABLE messages (
                   message_uuid TEXT PRIMARY KEY,
                   provider TEXT NOT NULL,
                   provider_message_id TEXT NOT NULL,
                   path TEXT NOT NULL,
                   thread_manifest TEXT,
                   thread_position INTEGER,
                   UNIQUE(provider, provider_message_id)
                 );
                 CREATE TABLE attachments (
                   message_uuid TEXT NOT NULL,
                   part_id TEXT NOT NULL,
                   path TEXT,
                   local INTEGER NOT NULL,
                   PRIMARY KEY(message_uuid, part_id)
                 );",
            )?;
            let transaction = connection.transaction()?;
            for path in sorted_files(&self.identities_dir())? {
                let identity: IdentityRecord = read_json(&path)?;
                require_version(identity.schema_version, "identity")?;
                transaction.execute(
                    "INSERT INTO messages(message_uuid, provider, provider_message_id, path)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        identity.message_id.to_string(),
                        identity.provider,
                        identity.provider_message_id,
                        self.data_dir()
                            .join(identity.message_id.to_string())
                            .display()
                            .to_string()
                    ],
                )?;
                let metadata_path = self
                    .data_dir()
                    .join(identity.message_id.to_string())
                    .join("metadata.json");
                if metadata_path.exists() {
                    let metadata: CanonicalMetadata = read_json(&metadata_path)?;
                    require_version(metadata.schema_version, "message metadata")?;
                    for attachment in metadata.attachments {
                        transaction.execute(
                            "INSERT INTO attachments(message_uuid, part_id, path, local)
                             VALUES (?1, ?2, ?3, ?4)",
                            params![
                                identity.message_id.to_string(),
                                attachment.id,
                                attachment.relative_path,
                                i64::from(attachment.local)
                            ],
                        )?;
                    }
                }
            }
            for path in sorted_files(&self.threads_dir())? {
                let manifest: ThreadManifest = read_json(&path)?;
                require_version(manifest.schema_version, "thread manifest")?;
                for (position, message_id) in manifest.messages.iter().enumerate() {
                    let updated = transaction.execute(
                        "UPDATE messages SET thread_manifest = ?1, thread_position = ?2
                         WHERE message_uuid = ?3",
                        params![
                            path.display().to_string(),
                            position as i64,
                            message_id.to_string()
                        ],
                    )?;
                    if updated != 1 {
                        return Err(error(format!(
                            "thread manifest references unknown message {message_id}"
                        )));
                    }
                }
            }
            transaction.commit()?;
            drop(connection);
            set_private_file_permissions(&temporary)?;
            fs::rename(&temporary, &target)?;
            Ok(())
        })();
        if result.is_err() {
            remove_if_exists(&temporary)?;
        }
        result
    }

    pub fn attachment_state(&self, message_id: Uuid, part_id: &str) -> Result<AttachmentState> {
        validate_part_id(part_id)?;
        let message_dir = self.data_dir().join(message_id.to_string());
        let metadata: CanonicalMetadata = read_json(&message_dir.join("metadata.json"))?;
        require_version(metadata.schema_version, "message metadata")?;
        let attachment = metadata
            .attachments
            .iter()
            .find(|attachment| attachment.id == part_id)
            .ok_or_else(|| {
                error(format!(
                    "message {message_id} has no attachment part {part_id}"
                ))
            })?;
        let relative_path = attachment_relative_path(&attachment.id, &attachment.filename);
        if attachment.local {
            if attachment.relative_path.as_deref() != Some(&relative_path) {
                return Err(error(
                    "local attachment path does not match canonical mapping",
                ));
            }
            let path = message_dir.join(relative_path);
            if !path.is_file() {
                return Err(error("local attachment file is missing"));
            }
            return Ok(AttachmentState::Local { path });
        }
        if attachment.relative_path.is_some() {
            return Err(error("remote attachment unexpectedly has a local path"));
        }
        let record: ProviderRecord =
            read_json(&self.provider_dir().join(format!("{message_id}.json")))?;
        require_version(record.schema_version, "provider message")?;
        let remote = record
            .remote_attachments
            .into_iter()
            .find(|attachment| attachment.part_id == part_id)
            .map(|attachment| RemoteAttachment {
                provider_attachment_id: attachment.provider_attachment_id,
                size: attachment.size,
            })
            .ok_or_else(|| error("remote attachment has no provider reference"))?;
        if remote.size != attachment.size {
            return Err(error("remote attachment size does not match metadata"));
        }
        Ok(AttachmentState::Remote(remote))
    }

    pub fn persist_attachment(
        &self,
        message_id: Uuid,
        part_id: &str,
        bytes: &[u8],
    ) -> Result<PathBuf> {
        let _lock = self.account_lock()?;
        let repository = Repository::open(self.root.clone())?;
        crate::integrity::prepare_account(&repository, self.account_id)?;
        let path = self.persist_attachment_unlocked(message_id, part_id, bytes)?;
        crate::integrity::commit_account(&repository, self.account_id)?;
        Ok(path)
    }

    pub(crate) fn persist_attachment_unlocked(
        &self,
        message_id: Uuid,
        part_id: &str,
        bytes: &[u8],
    ) -> Result<PathBuf> {
        self.create_layout()?;
        let remote = match self.attachment_state(message_id, part_id)? {
            AttachmentState::Local { path } => return Ok(path),
            AttachmentState::Remote(remote) => remote,
        };
        if bytes.len() as u64 != remote.size {
            return Err(error(
                "fetched attachment size does not match provider metadata",
            ));
        }

        let message_dir = self.data_dir().join(message_id.to_string());
        let mut metadata: CanonicalMetadata = read_json(&message_dir.join("metadata.json"))?;
        require_version(metadata.schema_version, "message metadata")?;
        let staging = self
            .staging_dir()
            .join(format!("attachment-{}", Uuid::new_v4()));
        create_private_dir(&staging)?;
        let result = (|| -> Result<PathBuf> {
            write_private(
                &staging.join("content.md"),
                &fs::read(message_dir.join("content.md"))?,
            )?;
            for attachment in &metadata.attachments {
                if attachment.local {
                    let relative = attachment_relative_path(&attachment.id, &attachment.filename);
                    if attachment.relative_path.as_deref() != Some(&relative) {
                        return Err(error(
                            "local attachment path does not match canonical mapping",
                        ));
                    }
                    let target = staging.join(&relative);
                    create_private_dir(target.parent().expect("attachment parent"))?;
                    write_private(&target, &fs::read(message_dir.join(relative))?)?;
                }
            }
            let attachment = metadata
                .attachments
                .iter_mut()
                .find(|attachment| attachment.id == part_id)
                .expect("attachment was checked before staging");
            let relative = attachment_relative_path(&attachment.id, &attachment.filename);
            let target = staging.join(&relative);
            create_private_dir(target.parent().expect("attachment parent"))?;
            write_private(&target, bytes)?;
            attachment.local = true;
            attachment.relative_path = Some(relative.clone());
            attachment.size = bytes.len() as u64;
            write_json(&staging.join("metadata.json"), &metadata)?;
            replace_dir(&staging, &message_dir)?;
            self.rebuild_index_unlocked()?;
            Ok(message_dir.join(relative))
        })();
        if let Err(cleanup) = fs::remove_dir_all(&staging)
            && cleanup.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("warning: failed to remove attachment staging directory: {cleanup}");
        }
        result
    }

    pub fn raw_path(&self, message_id: Uuid) -> PathBuf {
        self.account_dir()
            .join("provider/raw")
            .join(format!("{message_id}.eml"))
    }

    pub fn message_path(&self, message_id: Uuid) -> PathBuf {
        self.data_dir().join(message_id.to_string())
    }

    pub fn message(&self, message_id: Uuid) -> Result<(CanonicalMetadata, String)> {
        let directory = self.message_path(message_id);
        let metadata: CanonicalMetadata = read_json(&directory.join("metadata.json"))?;
        require_version(metadata.schema_version, "message metadata")?;
        if metadata.id != message_id {
            return Err(error("message metadata identity does not match its path"));
        }
        Ok((metadata, fs::read_to_string(directory.join("content.md"))?))
    }

    pub fn context_ids(&self, message_id: Uuid) -> Result<Vec<Uuid>> {
        for path in sorted_files(&self.threads_dir())? {
            let manifest: ThreadManifest = read_json(&path)?;
            require_version(manifest.schema_version, "thread manifest")?;
            if manifest.messages.contains(&message_id) {
                return Ok(manifest.messages);
            }
        }
        Err(error(format!(
            "message has no thread context: {message_id}"
        )))
    }

    pub(crate) fn persist_raw_unlocked(&self, message_id: Uuid, bytes: &[u8]) -> Result<PathBuf> {
        self.provider_message_id(message_id)?;
        let path = self.raw_path(message_id);
        if path.is_file() {
            return Ok(path);
        }
        create_private_dir(path.parent().expect("raw parent"))?;
        write_private(&path.with_extension("eml.tmp"), bytes)?;
        fs::rename(path.with_extension("eml.tmp"), &path)?;
        Ok(path)
    }

    pub(crate) fn message_id_for_provider(
        &self,
        provider_message_id: &str,
    ) -> Result<Option<Uuid>> {
        if !self.identities_dir().is_dir() {
            return Ok(None);
        }
        for path in sorted_files(&self.identities_dir())? {
            let identity: IdentityRecord = read_json(&path)?;
            require_version(identity.schema_version, "identity")?;
            if identity.provider == self.provider
                && identity.provider_message_id == provider_message_id
            {
                return Ok(Some(identity.message_id));
            }
        }
        Ok(None)
    }

    pub(crate) fn provider_message_id(&self, message_id: Uuid) -> Result<String> {
        let record: ProviderRecord =
            read_json(&self.provider_dir().join(format!("{message_id}.json")))?;
        require_version(record.schema_version, "provider message")?;
        Ok(record.provider_message_id)
    }

    pub(crate) fn identity_provider_message_id(&self, message_id: Uuid) -> Result<String> {
        let identity: IdentityRecord =
            read_json(&self.identities_dir().join(format!("{message_id}.json")))?;
        require_version(identity.schema_version, "identity")?;
        if identity.message_id != message_id || identity.provider != self.provider {
            return Err(error("identity record does not match message/account"));
        }
        Ok(identity.provider_message_id)
    }

    fn message_id(&self, provider: &str, provider_message_id: &str) -> Result<Uuid> {
        for path in sorted_files(&self.identities_dir())? {
            let identity: IdentityRecord = read_json(&path)?;
            require_version(identity.schema_version, "identity")?;
            if identity.provider == provider && identity.provider_message_id == provider_message_id
            {
                return Ok(identity.message_id);
            }
        }
        let message_id = Uuid::now_v7();
        write_json_atomic(
            &self.identities_dir().join(format!("{message_id}.json")),
            &IdentityRecord {
                schema_version: SCHEMA_VERSION,
                provider: provider.to_owned(),
                provider_message_id: provider_message_id.to_owned(),
                message_id,
            },
        )?;
        Ok(message_id)
    }

    fn write_thread_manifest(&self, manifest: ThreadManifest) -> Result<()> {
        let path = self
            .threads_dir()
            .join(format!("{}.json", manifest.messages[0]));
        for existing in sorted_files(&self.threads_dir())? {
            let previous: ThreadManifest = read_json(&existing)?;
            if previous.provider == manifest.provider
                && previous.provider_thread_id == manifest.provider_thread_id
                && existing != path
            {
                remove_if_exists(&existing)?;
            }
        }
        write_json_atomic(&path, &manifest)
    }

    fn create_layout(&self) -> Result<()> {
        if !self.account_dir().join("account.toml").is_file() {
            return Err(error("account state no longer exists"));
        }
        for path in [
            self.data_dir(),
            self.identities_dir(),
            self.provider_dir(),
            self.threads_dir(),
            self.diagnostics_dir(),
            self.staging_dir(),
        ] {
            create_private_dir(&path)?;
        }
        Ok(())
    }

    fn account_lock(&self) -> Result<MutationLock> {
        MutationLock::acquire(
            self.root
                .join(".bit-mail/locks/accounts")
                .join(format!("{}.lock", self.account_id)),
        )
    }

    fn account_dir(&self) -> PathBuf {
        self.root
            .join(".bit-mail/accounts")
            .join(self.account_id.to_string())
    }

    fn data_dir(&self) -> PathBuf {
        self.root
            .join("data")
            .join(self.account_id.to_string())
            .join("messages")
    }

    fn identities_dir(&self) -> PathBuf {
        self.account_dir().join("identities/messages")
    }

    fn provider_dir(&self) -> PathBuf {
        self.account_dir().join("provider/messages")
    }

    fn threads_dir(&self) -> PathBuf {
        self.account_dir().join("threads")
    }

    fn diagnostics_dir(&self) -> PathBuf {
        self.account_dir().join("diagnostics")
    }

    fn staging_dir(&self) -> PathBuf {
        self.account_dir().join("staging")
    }
}

fn validate_thread(thread: &ThreadInput) -> Result<()> {
    if thread.provider.trim().is_empty()
        || thread.provider_thread_id.trim().is_empty()
        || thread.messages.is_empty()
    {
        return Err(error(
            "provider, thread ID, and at least one message are required",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for message in &thread.messages {
        if message.provider_message_id.trim().is_empty()
            || message.provider_thread_id != thread.provider_thread_id
            || !seen.insert(&message.provider_message_id)
        {
            return Err(error(
                "thread contains an invalid or duplicate provider message",
            ));
        }
        if message.provider_source.is_null() {
            return Err(error("provider source must be retained"));
        }
        let mut part_ids = std::collections::HashSet::new();
        validate_parts(&message.parts, &mut part_ids)?;
    }
    Ok(())
}

fn validate_parts<'a>(
    parts: &'a [MimePartInput],
    seen: &mut std::collections::HashSet<&'a str>,
) -> Result<()> {
    for part in parts {
        validate_part_id(&part.id)?;
        if !seen.insert(&part.id) {
            return Err(error(format!("duplicate MIME part ID: {}", part.id)));
        }
        validate_parts(&part.parts, seen)?;
    }
    Ok(())
}

fn normalize_parts(parts: &[MimePartInput]) -> Result<Normalized> {
    let mut attachments = Vec::new();
    let mut diagnostics = Vec::new();
    let bodies = collect_parts(parts, &mut attachments, &mut diagnostics)?
        .into_iter()
        .filter_map(|body| match body {
            Body::Plain(text) => Some(text),
            Body::Html(source) => match htmd::convert(&source) {
                Ok(markdown) => Some(markdown),
                Err(problem) => {
                    diagnostics.push(Diagnostic {
                        severity: "warning",
                        stage: "html_to_markdown",
                        code: "html_conversion_failed",
                        detail: problem.to_string(),
                    });
                    None
                }
            },
        })
        .collect::<Vec<_>>();
    let content = if bodies.iter().any(|body| !body.trim().is_empty()) {
        canonical_content(bodies)
    } else {
        diagnostics.push(Diagnostic {
            severity: "warning",
            stage: "normalization",
            code: "no_readable_body",
            detail: "no non-attachment text body could be recovered".to_owned(),
        });
        "[Message content could not be normalized.]\n".to_owned()
    };
    Ok(Normalized {
        content,
        attachments,
        diagnostics,
    })
}

fn collect_parts(
    parts: &[MimePartInput],
    attachments: &mut Vec<(AttachmentMetadata, Option<Vec<u8>>)>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Vec<Body>> {
    let mut bodies = Vec::new();
    for part in parts {
        validate_part_id(&part.id)?;
        if !part.parts.is_empty() {
            let children = part
                .parts
                .iter()
                .map(|child| collect_parts(std::slice::from_ref(child), attachments, diagnostics))
                .collect::<Result<Vec<_>>>()?;
            if media_type(&part.mime_type).eq_ignore_ascii_case("multipart/alternative") {
                if let Some(selected) = children
                    .iter()
                    .find(|candidate| {
                        candidate.iter().any(
                            |body| matches!(body, Body::Plain(text) if !text.trim().is_empty()),
                        )
                    })
                    .or_else(|| {
                        children.iter().find(|candidate| {
                            candidate.iter().any(|body| match body {
                                Body::Plain(text) | Body::Html(text) => !text.trim().is_empty(),
                            })
                        })
                    })
                {
                    bodies.extend(selected.iter().cloned());
                }
            } else {
                bodies.extend(children.into_iter().flatten());
            }
            continue;
        }
        let media_type = media_type(&part.mime_type);
        let disposition = header(&part.headers, "content-disposition").unwrap_or_default();
        let is_attachment = part.filename.is_some()
            || part.remote.is_some()
            || disposition.to_ascii_lowercase().contains("attachment")
            || !media_type
                .get(..5)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("text/"));
        let decoded = part
            .body
            .as_deref()
            .map(|bytes| decode_transfer(bytes, part.transfer_encoding, &part.id, diagnostics));
        if is_attachment {
            let filename = part
                .filename
                .clone()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("attachment-{}", part.id));
            if decoded.is_none() && part.remote.is_none() {
                diagnostics.push(Diagnostic {
                    severity: "warning",
                    stage: "attachment",
                    code: "attachment_content_unavailable",
                    detail: format!(
                        "part {} has neither bytes nor a provider reference",
                        part.id
                    ),
                });
            }
            let relative_path = decoded
                .as_ref()
                .map(|_| attachment_relative_path(&part.id, &filename));
            let size = decoded.as_ref().map_or_else(
                || part.remote.as_ref().map_or(0, |remote| remote.size),
                |b| b.len() as u64,
            );
            attachments.push((
                AttachmentMetadata {
                    id: part.id.clone(),
                    filename,
                    media_type: media_type.to_owned(),
                    size,
                    local: decoded.is_some(),
                    relative_path,
                },
                decoded,
            ));
        } else if let Some(bytes) = decoded {
            let text = decode_charset(&bytes, &part.mime_type, &part.id, diagnostics);
            if media_type.eq_ignore_ascii_case("text/plain") {
                bodies.push(Body::Plain(text));
            } else if media_type.eq_ignore_ascii_case("text/html") {
                bodies.push(Body::Html(text));
            }
        }
    }
    Ok(bodies)
}

fn remote_attachment_references(parts: &[MimePartInput]) -> Vec<ProviderAttachmentReference> {
    let mut references = Vec::new();
    for part in parts {
        if let Some(remote) = &part.remote {
            references.push(ProviderAttachmentReference {
                part_id: part.id.clone(),
                provider_attachment_id: remote.provider_attachment_id.clone(),
                size: remote.size,
            });
        }
        references.extend(remote_attachment_references(&part.parts));
    }
    references
}

fn decode_transfer(
    bytes: &[u8],
    encoding: TransferEncoding,
    part_id: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<u8> {
    let decoded = match encoding {
        TransferEncoding::None => Some(bytes.to_vec()),
        TransferEncoding::Base64 => mail_parser::decoders::base64::base64_decode(bytes),
        TransferEncoding::QuotedPrintable => {
            mail_parser::decoders::quoted_printable::quoted_printable_decode(bytes)
        }
    };
    decoded.unwrap_or_else(|| {
        diagnostics.push(Diagnostic {
            severity: "warning",
            stage: "transfer_decode",
            code: "invalid_transfer_encoding",
            detail: format!("part {part_id} retained undecoded bytes"),
        });
        bytes.to_vec()
    })
}

fn decode_charset(
    bytes: &[u8],
    mime_type: &str,
    part_id: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> String {
    let charset = mime_type
        .split(';')
        .skip(1)
        .find_map(|parameter| {
            let (name, value) = parameter.trim().split_once('=')?;
            name.eq_ignore_ascii_case("charset")
                .then(|| value.trim_matches(['"', '\'']).to_owned())
        })
        .unwrap_or_else(|| "utf-8".to_owned());
    if charset.eq_ignore_ascii_case("utf-8") || charset.eq_ignore_ascii_case("us-ascii") {
        let valid = if charset.eq_ignore_ascii_case("us-ascii") {
            bytes.is_ascii()
        } else {
            std::str::from_utf8(bytes).is_ok()
        };
        if !valid {
            diagnostics.push(Diagnostic {
                severity: "warning",
                stage: "charset_decode",
                code: "invalid_charset_bytes",
                detail: format!("part {part_id} contained bytes invalid for {charset}"),
            });
        }
        String::from_utf8_lossy(bytes).into_owned()
    } else if let Some(decoder) =
        mail_parser::decoders::charsets::map::charset_decoder(charset.as_bytes())
    {
        decoder(bytes)
    } else {
        diagnostics.push(Diagnostic {
            severity: "warning",
            stage: "charset_decode",
            code: "unknown_charset",
            detail: format!("part {part_id} used unknown charset {charset}"),
        });
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn canonical_content(bodies: Vec<String>) -> String {
    let joined = bodies
        .into_iter()
        .map(|body| body.replace("\r\n", "\n").replace('\r', "\n"))
        .filter(|body| !body.trim().is_empty())
        .map(|body| body.trim_end_matches('\n').to_owned())
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("{joined}\n")
}

fn media_type(value: &str) -> &str {
    value.split(';').next().unwrap_or(value).trim()
}

fn header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn validate_part_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(error(
            "MIME part ID is not safe for stable attachment paths",
        ));
    }
    Ok(())
}

fn safe_filename(filename: &str) -> String {
    let mut safe: String = filename
        .chars()
        .take(100)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    while safe.starts_with('.') {
        safe.remove(0);
    }
    if safe.is_empty() {
        "attachment".to_owned()
    } else {
        safe
    }
}

fn attachment_relative_path(part_id: &str, filename: &str) -> String {
    format!("attachments/{part_id}--{}", safe_filename(filename))
}

fn sorted_files(directory: &Path) -> Result<Vec<PathBuf>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    write_private(path, &bytes)
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    write_json(&temporary, value)?;
    if let Err(problem) = fs::rename(&temporary, path) {
        remove_if_exists(&temporary)?;
        return Err(problem.into());
    }
    Ok(())
}

fn replace_dir(source: &Path, target: &Path) -> Result<()> {
    if !target.exists() {
        fs::rename(source, target)?;
        return Ok(());
    }
    let backup = target.with_extension(format!("backup-{}", Uuid::new_v4()));
    fs::rename(target, &backup)?;
    if let Err(problem) = fs::rename(source, target) {
        fs::rename(&backup, target)?;
        return Err(problem.into());
    }
    fs::remove_dir_all(backup)?;
    Ok(())
}

fn remove_if_exists(path: &Path) -> Result<()> {
    if let Err(problem) = fs::remove_file(path)
        && problem.kind() != std::io::ErrorKind::NotFound
    {
        return Err(problem.into());
    }
    Ok(())
}

fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes)?;
    set_private_file_permissions(path)
}

fn set_private_file_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(())
}

fn require_version(version: u32, kind: &str) -> Result<()> {
    if version == SCHEMA_VERSION {
        Ok(())
    } else {
        Err(error(format!(
            "unsupported {kind} schema version {version}"
        )))
    }
}

fn error(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(message.into()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{GitIgnorePolicy, NewAccount};
    use mail_parser::{MessageParser, MimeHeaders, PartType};

    fn store() -> (tempfile::TempDir, Repository, AccountConfig, CanonicalStore) {
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
        let store = CanonicalStore::new(&repository, &account).unwrap();
        (directory, repository, account, store)
    }

    fn message(
        provider_message_id: &str,
        thread_id: &str,
        parts: Vec<MimePartInput>,
    ) -> MessageInput {
        MessageInput {
            provider_message_id: provider_message_id.to_owned(),
            provider_thread_id: thread_id.to_owned(),
            received_at_ms: 1_700_000_000_000,
            sent_at_ms: Some(1_699_999_999_000),
            subject: Some("Subject".to_owned()),
            from: vec![Address {
                name: Some("Sender".to_owned()),
                address: "sender@example.com".to_owned(),
            }],
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            reply_to: Vec::new(),
            rfc_message_id: Some("<message@example.com>".to_owned()),
            flags: MailboxFlags {
                inbox: true,
                unread: true,
                ..Default::default()
            },
            parts,
            provider_source: serde_json::json!({"id": provider_message_id}),
        }
    }

    fn text_part(id: &str, mime_type: &str, body: &[u8]) -> MimePartInput {
        MimePartInput {
            id: id.to_owned(),
            mime_type: mime_type.to_owned(),
            headers: BTreeMap::new(),
            filename: None,
            transfer_encoding: TransferEncoding::None,
            body: Some(body.to_vec()),
            remote: None,
            parts: Vec::new(),
        }
    }

    #[test]
    fn canonical_storage_prefers_plain_text_and_maps_attachments_safely() {
        let (_directory, repository, account, store) = store();
        let mut encoded = text_part("0.1", "text/plain; charset=iso-8859-1", b"Ol=E1\r\n");
        encoded.transfer_encoding = TransferEncoding::QuotedPrintable;
        let local = MimePartInput {
            id: "0.3".to_owned(),
            mime_type: "application/pdf".to_owned(),
            headers: BTreeMap::new(),
            filename: Some("../../private report.pdf".to_owned()),
            transfer_encoding: TransferEncoding::None,
            body: Some(b"pdf".to_vec()),
            remote: None,
            parts: Vec::new(),
        };
        let remote = MimePartInput {
            id: "0.4".to_owned(),
            mime_type: "image/png".to_owned(),
            headers: BTreeMap::new(),
            filename: Some("photo.png".to_owned()),
            transfer_encoding: TransferEncoding::None,
            body: None,
            remote: Some(RemoteAttachment {
                provider_attachment_id: "remote-1".to_owned(),
                size: 42,
            }),
            parts: Vec::new(),
        };
        let input_parts = vec![
            MimePartInput {
                id: "0".to_owned(),
                mime_type: "multipart/alternative".to_owned(),
                headers: BTreeMap::new(),
                filename: None,
                transfer_encoding: TransferEncoding::None,
                body: None,
                remote: None,
                parts: vec![
                    encoded,
                    text_part(
                        "0.2",
                        "text/html",
                        b"<p>ignored <a href='https://example.com'>link</a></p>",
                    ),
                ],
            },
            local,
            remote,
        ];
        let normalized = normalize_parts(&input_parts).unwrap();
        assert!(
            normalized.diagnostics.is_empty(),
            "{:?}",
            normalized.diagnostics
        );
        let ids = store
            .materialize_thread(&ThreadInput {
                provider: "gmail".to_owned(),
                provider_thread_id: "thread-1".to_owned(),
                messages: vec![message("message-1", "thread-1", input_parts)],
            })
            .unwrap();
        let message_dir = repository
            .data_dir(account.id)
            .join("messages")
            .join(ids[0].to_string());
        assert_eq!(
            fs::read_to_string(message_dir.join("content.md")).unwrap(),
            "Olá\n"
        );
        let metadata: CanonicalMetadata = read_json(&message_dir.join("metadata.json")).unwrap();
        assert_eq!(metadata.normalization, NormalizationStatus::Complete);
        assert_eq!(metadata.attachments.len(), 2);
        assert!(
            metadata.attachments[0]
                .relative_path
                .as_deref()
                .unwrap()
                .contains("private_report.pdf")
        );
        assert!(
            message_dir
                .join(metadata.attachments[0].relative_path.as_ref().unwrap())
                .is_file()
        );
        assert!(!metadata.attachments[1].local);
        assert!(metadata.attachments[1].relative_path.is_none());
        assert_eq!(
            store.attachment_state(ids[0], "0.4").unwrap(),
            AttachmentState::Remote(RemoteAttachment {
                provider_attachment_id: "remote-1".to_owned(),
                size: 42
            })
        );
        assert!(store.raw_path(ids[0]).ends_with(format!("{}.eml", ids[0])));
        assert!(store.staging_dir().starts_with(store.account_dir()));
        assert!(!store.staging_dir().starts_with(store.data_dir()));
    }

    #[test]
    fn fetched_attachment_survives_rematerialization_and_is_indexed() {
        let (_directory, repository, account, store) = store();
        let remote = MimePartInput {
            id: "1".to_owned(),
            mime_type: "application/octet-stream".to_owned(),
            headers: BTreeMap::new(),
            filename: Some("file.bin".to_owned()),
            transfer_encoding: TransferEncoding::None,
            body: None,
            remote: Some(RemoteAttachment {
                provider_attachment_id: "remote-1".to_owned(),
                size: 3,
            }),
            parts: Vec::new(),
        };
        let thread = ThreadInput {
            provider: "gmail".to_owned(),
            provider_thread_id: "thread-1".to_owned(),
            messages: vec![message(
                "message-1",
                "thread-1",
                vec![text_part("0", "text/plain", b"body"), remote],
            )],
        };
        let id = store.materialize_thread(&thread).unwrap()[0];

        let path = store.persist_attachment(id, "1", b"abc").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"abc");
        assert_eq!(
            store.attachment_state(id, "1").unwrap(),
            AttachmentState::Local { path: path.clone() }
        );
        assert_eq!(store.persist_attachment(id, "1", b"ignored").unwrap(), path);
        assert_eq!(fs::read(&path).unwrap(), b"abc");
        assert_eq!(store.materialize_thread(&thread).unwrap(), vec![id]);
        assert_eq!(fs::read(&path).unwrap(), b"abc");
        assert_eq!(
            store.attachment_state(id, "1").unwrap(),
            AttachmentState::Local { path: path.clone() }
        );

        let index = Connection::open(
            repository
                .root()
                .join(".bit-mail/accounts")
                .join(account.id.to_string())
                .join("index.sqlite"),
        )
        .unwrap();
        let indexed: (String, i64) = index
            .query_row(
                "SELECT path, local FROM attachments WHERE message_uuid = ?1 AND part_id = '1'",
                [id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(indexed, ("attachments/1--file.bin".to_owned(), 1));
        assert!(store.attachment_state(id, "missing").is_err());

        let mut delivered = thread.clone();
        delivered.messages[0].parts[1].body = Some(b"xyz".to_vec());
        store.materialize_thread(&delivered).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"xyz");
        fs::remove_file(&path).unwrap();
        assert!(store.materialize_thread(&thread).is_err());
    }

    #[test]
    fn mime_structure_selects_alternatives_and_preserves_mixed_body_order() {
        let alternative = MimePartInput {
            id: "0".to_owned(),
            mime_type: "Multipart/Alternative".to_owned(),
            headers: BTreeMap::new(),
            filename: None,
            transfer_encoding: TransferEncoding::None,
            body: None,
            remote: None,
            parts: vec![
                text_part("0.1", "Text/Plain", b"chosen"),
                text_part("0.2", "TEXT/HTML", b"<p>ignored</p>"),
            ],
        };
        assert_eq!(normalize_parts(&[alternative]).unwrap().content, "chosen\n");

        let mixed = normalize_parts(&[
            text_part("1", "Text/Plain", b"first"),
            text_part(
                "2",
                "TEXT/HTML",
                b"<p>second <a href='https://example.com'>link</a></p>",
            ),
        ])
        .unwrap();
        assert_eq!(
            mixed.content,
            "first\n\nsecond [link](https://example.com)\n"
        );
        assert!(mixed.attachments.is_empty());
    }

    #[test]
    fn identity_and_thread_order_survive_cache_and_index_rebuilds() {
        let (_directory, repository, account, store) = store();
        let mut read = message(
            "read",
            "thread-1",
            vec![text_part("0", "text/plain", b"read")],
        );
        read.flags.unread = false;
        let mut archived = message(
            "archived",
            "thread-1",
            vec![text_part("0", "text/plain", b"archived")],
        );
        archived.flags = MailboxFlags::default();
        let mut sent = message(
            "sent",
            "thread-1",
            vec![text_part("0", "text/plain", b"sent")],
        );
        sent.flags = MailboxFlags {
            sent: true,
            ..Default::default()
        };
        let thread = ThreadInput {
            provider: "gmail".to_owned(),
            provider_thread_id: "thread-1".to_owned(),
            messages: vec![
                message(
                    "unread",
                    "thread-1",
                    vec![text_part("0", "text/plain", b"unread")],
                ),
                read,
                archived,
                sent,
            ],
        };
        let first_ids = store.materialize_thread(&thread).unwrap();
        crate::recovery::cache_rebuild(&repository, &account).unwrap();
        let second_ids = store.materialize_thread(&thread).unwrap();
        let messages = repository.data_dir(account.id).join("messages");
        assert_eq!(first_ids, second_ids);

        let index = Connection::open(
            repository
                .root()
                .join(".bit-mail/accounts")
                .join(account.id.to_string())
                .join("index.sqlite"),
        )
        .unwrap();
        let indexed: Vec<String> = index
            .prepare("SELECT message_uuid FROM messages ORDER BY thread_position")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert_eq!(
            indexed,
            first_ids.iter().map(Uuid::to_string).collect::<Vec<_>>()
        );
        assert_eq!(fs::read_dir(&messages).unwrap().count(), 4);
        let flags = second_ids
            .iter()
            .map(|id| {
                read_json::<CanonicalMetadata>(&messages.join(id.to_string()).join("metadata.json"))
                    .unwrap()
                    .flags
            })
            .collect::<Vec<_>>();
        assert_eq!(
            flags,
            vec![
                MailboxFlags {
                    inbox: true,
                    unread: true,
                    ..Default::default()
                },
                MailboxFlags {
                    inbox: true,
                    ..Default::default()
                },
                MailboxFlags::default(),
                MailboxFlags {
                    sent: true,
                    ..Default::default()
                },
            ]
        );
    }

    #[test]
    fn account_provider_bounds_canonical_identity() {
        let (_directory, repository, account, store) = store();
        let error = store
            .materialize_thread(&ThreadInput {
                provider: "not-gmail".to_owned(),
                provider_thread_id: "thread-1".to_owned(),
                messages: vec![message(
                    "message-1",
                    "thread-1",
                    vec![text_part("0", "text/plain", b"body")],
                )],
            })
            .unwrap_err();

        assert!(error.to_string().contains("does not match"));
        assert!(
            !repository
                .root()
                .join(".bit-mail/accounts")
                .join(account.id.to_string())
                .join("identities/messages")
                .exists()
        );
        assert!(!repository.data_dir(account.id).join("messages").exists());
    }

    #[test]
    fn failed_index_rebuild_preserves_the_last_valid_index() {
        let (_directory, repository, account, store) = store();
        store
            .materialize_thread(&ThreadInput {
                provider: "gmail".to_owned(),
                provider_thread_id: "thread-1".to_owned(),
                messages: vec![message(
                    "message-1",
                    "thread-1",
                    vec![text_part("0", "text/plain", b"body")],
                )],
            })
            .unwrap();
        let account_dir = repository
            .root()
            .join(".bit-mail/accounts")
            .join(account.id.to_string());
        let index_path = account_dir.join("index.sqlite");
        let previous = fs::read(&index_path).unwrap();
        fs::write(account_dir.join("identities/messages/broken.json"), b"{").unwrap();

        assert!(store.rebuild_index().is_err());
        assert_eq!(fs::read(&index_path).unwrap(), previous);
        let index = Connection::open(index_path).unwrap();
        assert_eq!(
            index
                .query_row("SELECT COUNT(*) FROM messages", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn malformed_input_is_published_as_partial_instead_of_dropped() {
        let (_directory, repository, account, store) = store();
        let mut part = text_part("0", "text/plain; charset=unknown", b"not base64!");
        part.transfer_encoding = TransferEncoding::Base64;
        let ids = store
            .materialize_thread(&ThreadInput {
                provider: "gmail".to_owned(),
                provider_thread_id: "thread-1".to_owned(),
                messages: vec![message("message-1", "thread-1", vec![part])],
            })
            .unwrap();
        let metadata: CanonicalMetadata = read_json(
            &repository
                .data_dir(account.id)
                .join("messages")
                .join(ids[0].to_string())
                .join("metadata.json"),
        )
        .unwrap();
        assert_eq!(metadata.normalization, NormalizationStatus::Partial);
        assert!(
            repository
                .root()
                .join(".bit-mail/accounts")
                .join(account.id.to_string())
                .join("diagnostics")
                .join(format!("{}.json", ids[0]))
                .is_file()
        );
    }

    #[test]
    fn invalid_charset_and_missing_attachment_content_are_diagnosed() {
        let (_directory, repository, account, store) = store();
        let missing = MimePartInput {
            id: "1".to_owned(),
            mime_type: "application/octet-stream".to_owned(),
            headers: BTreeMap::new(),
            filename: Some("missing.bin".to_owned()),
            transfer_encoding: TransferEncoding::None,
            body: None,
            remote: None,
            parts: Vec::new(),
        };
        let id = store
            .materialize_thread(&ThreadInput {
                provider: "gmail".to_owned(),
                provider_thread_id: "thread-1".to_owned(),
                messages: vec![message(
                    "message-1",
                    "thread-1",
                    vec![
                        text_part("0", "text/plain; charset=us-ascii", b"bad \xff"),
                        missing,
                    ],
                )],
            })
            .unwrap()[0];
        let message_dir = repository
            .data_dir(account.id)
            .join("messages")
            .join(id.to_string());
        let metadata: CanonicalMetadata = read_json(&message_dir.join("metadata.json")).unwrap();
        assert_eq!(metadata.normalization, NormalizationStatus::Partial);
        assert!(
            fs::read_to_string(message_dir.join("content.md"))
                .unwrap()
                .contains('\u{fffd}')
        );
        let diagnostics: Value = read_json(
            &repository
                .root()
                .join(".bit-mail/accounts")
                .join(account.id.to_string())
                .join("diagnostics")
                .join(format!("{id}.json")),
        )
        .unwrap();
        assert_eq!(
            diagnostics["diagnostics"][0]["code"],
            "invalid_charset_bytes"
        );
        assert_eq!(
            diagnostics["diagnostics"][1]["code"],
            "attachment_content_unavailable"
        );
    }

    #[test]
    fn rfc822_parser_decodes_multipart_content_and_attachments() {
        let raw = b"From: sender@example.com\r\nContent-Type: multipart/mixed; boundary=x\r\n\r\n--x\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>Hello <a href=\"https://example.com\">there</a></p>\r\n--x\r\nContent-Type: application/octet-stream; name=file.bin\r\nContent-Disposition: attachment; filename=file.bin\r\nContent-Transfer-Encoding: base64\r\n\r\nYWJj\r\n--x--\r\n";
        let parsed = MessageParser::default().parse(raw).unwrap();
        assert!(parsed.parts.iter().all(|part| !part.is_encoding_problem));
        assert!(matches!(
            &parsed.parts[parsed.html_body[0] as usize].body,
            PartType::Html(body) if body.contains("https://example.com")
        ));
        let attachment = parsed.attachments().next().unwrap();
        assert_eq!(attachment.attachment_name(), Some("file.bin"));
        assert!(matches!(&attachment.body, PartType::Binary(body) if body.as_ref() == b"abc"));
    }

    #[test]
    fn unsafe_part_ids_are_rejected_before_writing_message_content() {
        let (_directory, _repository, _account, store) = store();
        let error = store
            .materialize_thread(&ThreadInput {
                provider: "gmail".to_owned(),
                provider_thread_id: "thread-1".to_owned(),
                messages: vec![message(
                    "message-1",
                    "thread-1",
                    vec![text_part("../../escape", "text/plain", b"bad")],
                )],
            })
            .unwrap_err();
        assert!(error.to_string().contains("not safe"));
    }
}
