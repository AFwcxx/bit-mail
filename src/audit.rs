use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use uuid::Uuid;

use crate::Result;

#[derive(Serialize)]
struct AuditEvent<'a> {
    schema_version: u32,
    id: Uuid,
    occurred_at_ms: u128,
    action: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<Uuid>,
    #[serde(skip_serializing_if = "slice_empty")]
    message_ids: &'a [Uuid],
    #[serde(skip_serializing_if = "Option::is_none")]
    selection: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    knowledge_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<&'a str>,
}

fn slice_empty(value: &&[Uuid]) -> bool {
    value.is_empty()
}

pub(crate) struct Details<'a> {
    pub account_id: Option<Uuid>,
    pub message_ids: &'a [Uuid],
    pub selection: Option<&'a str>,
    pub knowledge_id: Option<Uuid>,
    pub value: Option<&'a str>,
}

pub(crate) fn append(directory: &Path, action: &str, details: Details<'_>) -> Result<()> {
    fs::create_dir_all(directory)?;
    set_private_directory(directory)?;
    let now = SystemTime::now();
    let formatted = httpdate::fmt_http_date(now);
    let parts: Vec<_> = formatted.split_whitespace().collect();
    let path = directory.join(format!("{}-{}.jsonl", parts[3], parts[2]));
    let mut line = serde_json::to_vec(&AuditEvent {
        schema_version: 1,
        id: Uuid::now_v7(),
        occurred_at_ms: now.duration_since(UNIX_EPOCH)?.as_millis(),
        action,
        account_id: details.account_id,
        message_ids: details.message_ids,
        selection: details.selection,
        knowledge_id: details.knowledge_id,
        value: details.value,
    })?;
    line.push(b'\n');
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    file.write_all(&line)?;
    file.sync_data()?;
    set_private_file(&path)
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
