use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Result, repository::Repository};

const SCHEMA_VERSION: u32 = 1;
const BUFFER_SIZE: usize = 128 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    domain: String,
    root: String,
    files: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntegrityMismatch {
    pub path: String,
    pub kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct FullValidation {
    pub schema_version: u32,
    pub repository_root: String,
    pub mismatches: Vec<IntegrityMismatch>,
}

pub(crate) fn prepare_account(repository: &Repository, account_id: Uuid) -> Result<()> {
    repository.require_integrity_ready()?;
    let path = account_manifest_path(repository, account_id);
    fail_on_mismatch(validate_scope(
        repository,
        &path,
        "bit-mail:account:v1",
        account_files(repository, account_id)?,
    )?)
}

pub(crate) fn commit_account(repository: &Repository, account_id: Uuid) -> Result<()> {
    require_manifest(&account_manifest_path(repository, account_id))?;
    rebuild_account(repository, account_id)
}

pub(crate) fn reset_account(repository: &Repository, account_id: Uuid) -> Result<()> {
    rebuild_account(repository, account_id)
}

pub(crate) fn prepare_account_triage(repository: &Repository, account_id: Uuid) -> Result<()> {
    bootstrap_account(repository, account_id)?;
    validate_subset(repository, account_id, |path| {
        path.ends_with("/account.toml")
            || path.contains("/work-items/")
            || path.contains("/selections/")
            || path.contains("/audit/")
    })
}

pub(crate) fn commit_account_triage(repository: &Repository, account_id: Uuid) -> Result<()> {
    commit_subset(repository, account_id, |path| {
        path.ends_with("/account.toml")
            || path.contains("/work-items/")
            || path.contains("/selections/")
            || path.contains("/audit/")
    })
}

pub(crate) fn prepare_account_knowledge(repository: &Repository, account_id: Uuid) -> Result<()> {
    bootstrap_account(repository, account_id)?;
    validate_subset(repository, account_id, |path| {
        path.ends_with("/account.toml")
            || path.starts_with(&format!("knowledge/accounts/{account_id}/"))
            || path.contains("/audit/")
    })
}

pub(crate) fn commit_account_knowledge(repository: &Repository, account_id: Uuid) -> Result<()> {
    commit_subset(repository, account_id, |path| {
        path.ends_with("/account.toml")
            || path.starts_with(&format!("knowledge/accounts/{account_id}/"))
            || path.contains("/audit/")
    })
}

pub(crate) fn bootstrap_account(repository: &Repository, account_id: Uuid) -> Result<()> {
    repository.require_integrity_ready()?;
    if !account_manifest_path(repository, account_id).is_file() {
        return Err(std::io::Error::other(
            "integrity manifest is missing; restore it from a trusted backup",
        )
        .into());
    }
    Ok(())
}

pub(crate) fn prepare_knowledge(repository: &Repository) -> Result<()> {
    prepare_scope(
        repository,
        &knowledge_manifest_path(repository),
        "bit-mail:knowledge:v1",
        global_knowledge_files(repository)?,
    )
}

pub(crate) fn commit_knowledge(repository: &Repository) -> Result<()> {
    require_manifest(&knowledge_manifest_path(repository))?;
    write_scope(
        repository,
        &knowledge_manifest_path(repository),
        "bit-mail:knowledge:v1",
        global_knowledge_files(repository)?,
    )
}

pub(crate) fn prepare_repository(repository: &Repository) -> Result<()> {
    prepare_scope(
        repository,
        &repository_manifest_path(repository),
        "bit-mail:repository-state:v1",
        repository_files(repository)?,
    )
}

pub(crate) fn commit_repository(repository: &Repository) -> Result<()> {
    require_manifest(&repository_manifest_path(repository))?;
    write_scope(
        repository,
        &repository_manifest_path(repository),
        "bit-mail:repository-state:v1",
        repository_files(repository)?,
    )
}

pub fn validate_account(
    repository: &Repository,
    account_id: Uuid,
) -> Result<Vec<IntegrityMismatch>> {
    repository.require_integrity_ready()?;
    validate_scope(
        repository,
        &account_manifest_path(repository, account_id),
        "bit-mail:account:v1",
        account_files(repository, account_id)?,
    )
}

pub fn validate_sensitive_scope(
    repository: &Repository,
    account_id: Uuid,
    message_ids: &[Uuid],
    selection: Option<&str>,
) -> Result<Vec<IntegrityMismatch>> {
    bootstrap_account(repository, account_id)?;
    let ids = message_ids.iter().map(Uuid::to_string).collect::<Vec<_>>();
    validate_subset_collect(repository, account_id, |path| {
        path.ends_with("/account.toml")
            || path.contains("/threads/")
            || path.contains("/audit/")
            || ids.iter().any(|id| {
                path.contains(&format!("/messages/{id}"))
                    || path.ends_with(&format!("/work-items/{id}.json"))
            })
            || selection.is_some_and(|name| path.ends_with(&format!("/selections/{name}.json")))
    })
}

pub(crate) fn validate_push_cleanup_scope(
    repository: &Repository,
    account_id: Uuid,
    message_ids: &[Uuid],
) -> Result<Vec<IntegrityMismatch>> {
    bootstrap_account(repository, account_id)?;
    let ids = message_ids.iter().map(Uuid::to_string).collect::<Vec<_>>();
    validate_subset_collect(repository, account_id, |path| push_cleanup_path(path, &ids))
}

pub(crate) fn commit_push_scope(
    repository: &Repository,
    account_id: Uuid,
    message_ids: &[Uuid],
) -> Result<()> {
    let ids = message_ids.iter().map(Uuid::to_string).collect::<Vec<_>>();
    commit_subset(repository, account_id, |path| push_cleanup_path(path, &ids))
}

fn push_cleanup_path(path: &str, ids: &[String]) -> bool {
    path.contains("/threads/")
        || path.contains("/audit/")
        || path.contains("/selections/")
        || path.ends_with("/provider-state.json")
        || ids.iter().any(|id| {
            path.contains(&format!("/messages/{id}"))
                || path.ends_with(&format!("/work-items/{id}.json"))
                || path.ends_with(&format!("/provider/raw/{id}.eml"))
                || path.ends_with(&format!("/diagnostics/{id}.json"))
        })
}

pub(crate) fn validate_repair_basis(
    repository: &Repository,
    account_id: Uuid,
    message_id: Uuid,
) -> Result<()> {
    validate_subset(repository, account_id, |path| {
        path.ends_with("/account.toml")
            || path.ends_with(&format!("/identities/messages/{message_id}.json"))
    })
}

pub(crate) fn validate_unaffected_repair_state(
    repository: &Repository,
    account_id: Uuid,
    affected: &[Uuid],
) -> Result<()> {
    let ids = affected.iter().map(Uuid::to_string).collect::<Vec<_>>();
    validate_subset(repository, account_id, |path| {
        !ids.iter().any(|id| {
            path.contains(&format!("/messages/{id}/"))
                || path.ends_with(&format!("/provider/messages/{id}.json"))
                || path.ends_with(&format!("/provider/raw/{id}.eml"))
                || path.ends_with(&format!("/diagnostics/{id}.json"))
                || path.ends_with(&format!("/threads/{id}.json"))
        })
    })
}

pub(crate) fn validate_preserved_account(repository: &Repository, account_id: Uuid) -> Result<()> {
    let account = format!(".bit-mail/accounts/{account_id}/");
    let knowledge = format!("knowledge/accounts/{account_id}/");
    validate_subset(repository, account_id, |path| {
        path == format!("{account}account.toml")
            || path.starts_with(&format!("{account}identities/"))
            || path.starts_with(&format!("{account}audit/"))
            || path.starts_with(&knowledge)
    })
}

pub(crate) fn validate_cache_rebuild_guard(
    repository: &Repository,
    account_id: Uuid,
) -> Result<()> {
    let account = format!(".bit-mail/accounts/{account_id}/");
    let knowledge = format!("knowledge/accounts/{account_id}/");
    validate_subset(repository, account_id, |path| {
        path == format!("{account}account.toml")
            || path.starts_with(&format!("{account}identities/"))
            || path.starts_with(&format!("{account}audit/"))
            || path.starts_with(&format!("{account}work-items/"))
            || path.starts_with(&knowledge)
    })
}

pub fn validate_full(repository: &Repository) -> Result<FullValidation> {
    let mut mismatches = Vec::new();
    let mut roots = Vec::new();
    for (path, domain, files) in [
        (
            repository_manifest_path(repository),
            "bit-mail:repository-state:v1",
            repository_files(repository)?,
        ),
        (
            knowledge_manifest_path(repository),
            "bit-mail:knowledge:v1",
            global_knowledge_files(repository)?,
        ),
    ] {
        let (mut found, root) = compare_scope(repository, &path, domain, files)?;
        mismatches.append(&mut found);
        roots.push((domain.to_owned(), root));
    }
    for account in repository.accounts()? {
        let domain = format!("bit-mail:account:v1:{}", account.id);
        let path = account_manifest_path(repository, account.id);
        let (mut found, root) = compare_scope(
            repository,
            &path,
            "bit-mail:account:v1",
            account_files(repository, account.id)?,
        )?;
        mismatches.append(&mut found);
        roots.push((domain, root));
    }
    for account_id in orphan_knowledge_ids(repository)? {
        let domain = format!("bit-mail:orphaned-account-knowledge:v1:{account_id}");
        let path = orphan_knowledge_manifest_path(repository, account_id);
        let (mut found, root) = compare_scope(
            repository,
            &path,
            "bit-mail:orphaned-account-knowledge:v1",
            orphan_knowledge_files(repository, account_id)?,
        )?;
        mismatches.append(&mut found);
        roots.push((domain, root));
    }
    roots.sort();
    let entries = roots
        .iter()
        .map(|(path, digest)| Entry {
            path: path.clone(),
            digest: digest.clone(),
        })
        .collect::<Vec<_>>();
    Ok(FullValidation {
        schema_version: SCHEMA_VERSION,
        repository_root: parent_digest("bit-mail:repository:v1", &entries),
        mismatches,
    })
}

pub fn rebuild_full(repository: &Repository) -> Result<FullValidation> {
    write_scope(
        repository,
        &repository_manifest_path(repository),
        "bit-mail:repository-state:v1",
        repository_files(repository)?,
    )?;
    write_scope(
        repository,
        &knowledge_manifest_path(repository),
        "bit-mail:knowledge:v1",
        global_knowledge_files(repository)?,
    )?;
    for account in repository.accounts()? {
        reset_account(repository, account.id)?;
    }
    for account_id in orphan_knowledge_ids(repository)? {
        commit_orphan_knowledge(repository, account_id)?;
    }
    validate_full(repository)
}

pub(crate) fn commit_orphan_knowledge(repository: &Repository, account_id: Uuid) -> Result<()> {
    write_scope(
        repository,
        &orphan_knowledge_manifest_path(repository, account_id),
        "bit-mail:orphaned-account-knowledge:v1",
        orphan_knowledge_files(repository, account_id)?,
    )
}

fn prepare_scope(
    repository: &Repository,
    path: &Path,
    domain: &str,
    files: Vec<PathBuf>,
) -> Result<()> {
    repository.require_integrity_ready()?;
    fail_on_mismatch(validate_scope(repository, path, domain, files)?)
}

fn validate_subset(
    repository: &Repository,
    account_id: Uuid,
    include: impl Fn(&str) -> bool,
) -> Result<()> {
    fail_on_mismatch(validate_subset_collect(repository, account_id, include)?)
}

fn validate_subset_collect(
    repository: &Repository,
    account_id: Uuid,
    include: impl Fn(&str) -> bool,
) -> Result<Vec<IntegrityMismatch>> {
    let path = account_manifest_path(repository, account_id);
    if !path.is_file() {
        return Err(std::io::Error::other(
            "integrity manifest is missing; run a normal command to bootstrap it",
        )
        .into());
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(path)?)?;
    if manifest.schema_version != SCHEMA_VERSION || manifest.domain != "bit-mail:account:v1" {
        return Err(
            std::io::Error::other("unsupported integrity manifest schema or domain").into(),
        );
    }
    let expected = manifest
        .files
        .iter()
        .filter(|entry| include(&entry.path))
        .cloned()
        .collect::<Vec<_>>();
    if expected.is_empty() {
        return Err(std::io::Error::other("integrity recovery basis is missing").into());
    }
    let files = account_files(repository, account_id)?
        .into_iter()
        .filter(|path| relative(repository.root(), path).is_ok_and(|path| include(&path)))
        .collect::<Vec<_>>();
    let actual = hash_files(repository.root(), files)?;
    let observed = actual
        .iter()
        .map(|entry| (&entry.path, &entry.digest))
        .collect::<BTreeMap<_, _>>();
    let expected_paths = expected
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut mismatches = expected
        .into_iter()
        .filter_map(|entry| match observed.get(&entry.path) {
            Some(digest) if **digest == entry.digest => None,
            Some(_) => Some(IntegrityMismatch {
                path: entry.path,
                kind: "modified",
            }),
            None => Some(IntegrityMismatch {
                path: entry.path,
                kind: "missing",
            }),
        })
        .collect::<Vec<_>>();
    for entry in actual {
        if !expected_paths.contains(&entry.path) {
            mismatches.push(IntegrityMismatch {
                path: entry.path,
                kind: "unexpected",
            });
        }
    }
    Ok(mismatches)
}

fn commit_subset(
    repository: &Repository,
    account_id: Uuid,
    include: impl Fn(&str) -> bool,
) -> Result<()> {
    let path = account_manifest_path(repository, account_id);
    require_manifest(&path)?;
    let mut manifest: Manifest = serde_json::from_slice(&fs::read(&path)?)?;
    let files = account_files(repository, account_id)?
        .into_iter()
        .filter(|path| relative(repository.root(), path).is_ok_and(|path| include(&path)))
        .collect::<Vec<_>>();
    manifest.files.retain(|entry| !include(&entry.path));
    manifest.files.extend(hash_files(repository.root(), files)?);
    manifest
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    manifest.root = scope_digest(&manifest.domain, &manifest.files);
    write_manifest(&path, &manifest)
}

fn require_manifest(path: &Path) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "integrity manifest disappeared during mutation; restore it from a trusted backup",
        )
        .into())
    }
}

fn fail_on_mismatch(mismatches: Vec<IntegrityMismatch>) -> Result<()> {
    if mismatches.is_empty() {
        return Ok(());
    }
    let summary = mismatches
        .iter()
        .take(8)
        .map(|item| format!("{} ({})", item.path, item.kind))
        .collect::<Vec<_>>()
        .join(", ");
    Err(std::io::Error::other(format!(
        "integrity mismatch: {summary}; run repair or cache rebuild"
    ))
    .into())
}

fn rebuild_account(repository: &Repository, account_id: Uuid) -> Result<()> {
    write_scope(
        repository,
        &account_manifest_path(repository, account_id),
        "bit-mail:account:v1",
        account_files(repository, account_id)?,
    )
}

fn write_scope(
    repository: &Repository,
    path: &Path,
    domain: &str,
    files: Vec<PathBuf>,
) -> Result<()> {
    let entries = hash_files(repository.root(), files)?;
    let manifest = Manifest {
        schema_version: SCHEMA_VERSION,
        domain: domain.to_owned(),
        root: scope_digest(domain, &entries),
        files: entries,
    };
    write_manifest(path, &manifest)
}

fn write_manifest(path: &Path, manifest: &Manifest) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("integrity path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(manifest)?)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn validate_scope(
    repository: &Repository,
    path: &Path,
    domain: &str,
    files: Vec<PathBuf>,
) -> Result<Vec<IntegrityMismatch>> {
    Ok(compare_scope(repository, path, domain, files)?.0)
}

fn compare_scope(
    repository: &Repository,
    path: &Path,
    domain: &str,
    files: Vec<PathBuf>,
) -> Result<(Vec<IntegrityMismatch>, String)> {
    if !path.is_file() {
        return Ok((
            vec![IntegrityMismatch {
                path: relative(repository.root(), path)?,
                kind: "missing manifest",
            }],
            String::new(),
        ));
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(path)?)?;
    if manifest.schema_version != SCHEMA_VERSION || manifest.domain != domain {
        return Err(
            std::io::Error::other("unsupported integrity manifest schema or domain").into(),
        );
    }
    let actual = hash_files(repository.root(), files)?;
    let expected = manifest
        .files
        .iter()
        .map(|entry| (&entry.path, &entry.digest))
        .collect::<BTreeMap<_, _>>();
    let observed = actual
        .iter()
        .map(|entry| (&entry.path, &entry.digest))
        .collect::<BTreeMap<_, _>>();
    let mut mismatches = Vec::new();
    for entry in &manifest.files {
        match observed.get(&entry.path) {
            None => mismatches.push(IntegrityMismatch {
                path: entry.path.clone(),
                kind: "missing",
            }),
            Some(digest) if **digest != entry.digest => mismatches.push(IntegrityMismatch {
                path: entry.path.clone(),
                kind: "modified",
            }),
            _ => {}
        }
    }
    for entry in &actual {
        if !expected.contains_key(&entry.path) {
            mismatches.push(IntegrityMismatch {
                path: entry.path.clone(),
                kind: "unexpected",
            });
        }
    }
    let root = scope_digest(domain, &actual);
    if root != manifest.root && mismatches.is_empty() {
        mismatches.push(IntegrityMismatch {
            path: relative(repository.root(), path)?,
            kind: "root",
        });
    }
    Ok((mismatches, root))
}

fn hash_files(root: &Path, mut files: Vec<PathBuf>) -> Result<Vec<Entry>> {
    files.sort();
    files.dedup();
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(files.len()));
    let workers = files.len().min(4);
    thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(index) else { break };
                    let value = hash_file(root, path).map(|entry| (index, entry));
                    results.lock().expect("integrity result lock").push(value);
                }
            });
        }
    });
    let mut values = results
        .into_inner()
        .expect("integrity result lock")
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    values.sort_by_key(|(index, _)| *index);
    Ok(values.into_iter().map(|(_, entry)| entry).collect())
}

fn hash_file(root: &Path, path: &Path) -> Result<Entry> {
    let relative = relative(root, path)?;
    let mut file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, b"bit-mail:file:v1");
    frame(&mut hasher, relative.as_bytes());
    hasher.update(&length.to_be_bytes());
    let mut buffer = [0_u8; BUFFER_SIZE];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Entry {
        path: relative,
        digest: hasher.finalize().to_hex().to_string(),
    })
}

fn parent_digest(domain: &str, entries: &[Entry]) -> String {
    let mut hasher = blake3::Hasher::new();
    frame(&mut hasher, domain.as_bytes());
    for entry in entries {
        frame(&mut hasher, entry.path.as_bytes());
        frame(&mut hasher, entry.digest.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

fn scope_digest(domain: &str, entries: &[Entry]) -> String {
    let mut objects: BTreeMap<(String, String), Vec<Entry>> = BTreeMap::new();
    for entry in entries {
        let (object_domain, identity) = object_identity(&entry.path);
        objects
            .entry((object_domain.to_owned(), identity))
            .or_default()
            .push(entry.clone());
    }
    let roots = objects
        .into_iter()
        .map(|((object_domain, identity), mut children)| {
            children.sort_by(|left, right| left.path.cmp(&right.path));
            Entry {
                path: format!("{object_domain}:{identity}"),
                digest: parent_digest(object_domain.as_str(), &children),
            }
        })
        .collect::<Vec<_>>();
    parent_digest(domain, &roots)
}

fn object_identity(path: &str) -> (&'static str, String) {
    for (segment, domain) in [
        ("/messages/", "bit-mail:message:v1"),
        ("/provider/raw/", "bit-mail:message:v1"),
        ("/diagnostics/", "bit-mail:message:v1"),
        ("/threads/", "bit-mail:thread:v1"),
        ("/work-items/", "bit-mail:work-item:v1"),
        ("/selections/", "bit-mail:selection:v1"),
        ("/audit/", "bit-mail:audit:v1"),
    ] {
        if let Some(rest) = path.split_once(segment).map(|(_, rest)| rest) {
            let identity = rest
                .split('/')
                .next()
                .unwrap_or(rest)
                .trim_end_matches(".json")
                .trim_end_matches(".jsonl")
                .trim_end_matches(".eml");
            return (domain, identity.to_owned());
        }
    }
    if path.starts_with("knowledge/") {
        return ("bit-mail:knowledge:v1", path.to_owned());
    }
    ("bit-mail:managed-object:v1", path.to_owned())
}

fn frame(hasher: &mut blake3::Hasher, value: &[u8]) {
    hasher.update(&(value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn account_files(repository: &Repository, account_id: Uuid) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect(
        &repository
            .root()
            .join(".bit-mail/accounts")
            .join(account_id.to_string()),
        &mut files,
        &["integrity", "staging"],
        &["index.sqlite"],
    )?;
    collect(
        &repository.root().join("data").join(account_id.to_string()),
        &mut files,
        &[],
        &[],
    )?;
    collect(
        &repository
            .root()
            .join("knowledge/accounts")
            .join(account_id.to_string()),
        &mut files,
        &[],
        &[],
    )?;
    Ok(files)
}

fn global_knowledge_files(repository: &Repository) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect(
        &repository.root().join("knowledge/global"),
        &mut files,
        &[],
        &[],
    )?;
    collect(
        &repository.root().join(".bit-mail/audit"),
        &mut files,
        &[],
        &[],
    )?;
    Ok(files)
}

fn orphan_knowledge_files(repository: &Repository, account_id: Uuid) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect(
        &repository
            .root()
            .join("knowledge/accounts")
            .join(account_id.to_string()),
        &mut files,
        &[],
        &[],
    )?;
    Ok(files)
}

fn orphan_knowledge_ids(repository: &Repository) -> Result<Vec<Uuid>> {
    let configured = repository
        .accounts()?
        .into_iter()
        .map(|account| account.id)
        .collect::<std::collections::HashSet<_>>();
    let root = repository.root().join("knowledge/accounts");
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut ids = fs::read_dir(root)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if !entry.path().is_dir() {
                return None;
            }
            entry.file_name().to_str()?.parse::<Uuid>().ok()
        })
        .filter(|id| !configured.contains(id))
        .collect::<Vec<_>>();
    ids.sort_unstable();
    Ok(ids)
}

fn repository_files(repository: &Repository) -> Result<Vec<PathBuf>> {
    let mut files = [
        ".bit-mail/repository.toml",
        ".bit-mail/config.toml",
        "AGENTS.md",
    ]
    .iter()
    .map(|path| repository.root().join(path))
    .filter(|path| path.is_file())
    .collect::<Vec<_>>();
    collect(&repository.root().join("skills"), &mut files, &[], &[])?;
    collect(&repository.root().join("templates"), &mut files, &[], &[])?;
    Ok(files)
}

fn collect(
    root: &Path,
    files: &mut Vec<PathBuf>,
    excluded_dirs: &[&str],
    excluded_files: &[&str],
) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !excluded_dirs.contains(&name.as_ref()) {
                collect(&path, files, excluded_dirs, excluded_files)?;
            }
        } else if path.is_file()
            && !excluded_files.contains(&name.as_ref())
            && !name.ends_with(".tmp")
            && !name.contains(".tmp-")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn relative(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)?
        .to_str()
        .ok_or_else(|| std::io::Error::other("managed path is not UTF-8"))?
        .replace('\\', "/"))
}

fn account_manifest_path(repository: &Repository, account_id: Uuid) -> PathBuf {
    repository
        .root()
        .join(".bit-mail/accounts")
        .join(account_id.to_string())
        .join("integrity/manifest.json")
}
fn knowledge_manifest_path(repository: &Repository) -> PathBuf {
    repository.root().join(".bit-mail/integrity/knowledge.json")
}
fn repository_manifest_path(repository: &Repository) -> PathBuf {
    repository
        .root()
        .join(".bit-mail/integrity/repository.json")
}

fn orphan_knowledge_manifest_path(repository: &Repository, account_id: Uuid) -> PathBuf {
    repository
        .root()
        .join(".bit-mail/integrity/orphaned-knowledge")
        .join(format!("{account_id}.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{GitIgnorePolicy, NewAccount};

    fn account(repository: &Repository, alias: &str) -> Uuid {
        repository
            .create_account(NewAccount {
                alias,
                provider: "gmail",
                provider_identity: None,
                credential_profile: None,
            })
            .unwrap()
            .id
    }

    #[test]
    fn validation_localizes_modified_missing_and_unexpected_files_by_account() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let first = account(&repository, "first");
        let second = account(&repository, "second");
        let second_config = repository
            .root()
            .join(".bit-mail/accounts")
            .join(second.to_string())
            .join("account.toml");
        fs::write(second_config, "tampered").unwrap();
        assert!(
            validate_account(&repository, first).unwrap().is_empty(),
            "an account scan must not read another account branch"
        );

        let root = repository
            .root()
            .join(".bit-mail/accounts")
            .join(first.to_string());
        let config = root.join("account.toml");
        fs::write(&config, "tampered").unwrap();
        fs::write(root.join("unexpected.json"), "{}").unwrap();
        let mismatches = validate_account(&repository, first).unwrap();
        assert!(
            mismatches
                .iter()
                .any(|item| item.kind == "modified" && item.path.ends_with("account.toml"))
        );
        assert!(
            mismatches
                .iter()
                .any(|item| item.kind == "unexpected" && item.path.ends_with("unexpected.json"))
        );
        fs::remove_file(config).unwrap();
        assert!(
            validate_account(&repository, first)
                .unwrap()
                .iter()
                .any(|item| item.kind == "missing")
        );
    }

    #[test]
    fn full_root_is_deterministic_and_excludes_sqlite_and_locks() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let id = account(&repository, "mail");
        let first = validate_full(&repository).unwrap();
        fs::write(
            repository
                .root()
                .join(".bit-mail/accounts")
                .join(id.to_string())
                .join("index.sqlite"),
            "disposable",
        )
        .unwrap();
        fs::write(
            repository.root().join(".bit-mail/locks/ignored.lock"),
            "transient",
        )
        .unwrap();
        let second = validate_full(&repository).unwrap();
        assert_eq!(first.repository_root, second.repository_root);
        assert!(second.mismatches.is_empty());
    }

    #[test]
    fn sensitive_scope_localizes_tampered_message_content() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let account_id = account(&repository, "mail");
        let message_id = Uuid::now_v7();
        let directory = repository
            .data_dir(account_id)
            .join("messages")
            .join(message_id.to_string());
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("content.md"), "trusted\n").unwrap();
        commit_account(&repository, account_id).unwrap();
        fs::write(directory.join("content.md"), "tampered\n").unwrap();
        assert!(
            validate_sensitive_scope(&repository, account_id, &[message_id], None)
                .unwrap()
                .iter()
                .any(|item| item.kind == "modified" && item.path.ends_with("content.md"))
        );
    }

    #[test]
    #[ignore = "manual large-file benchmark"]
    fn benchmark_large_file_streaming_against_mmap_parallel() {
        use std::time::Instant;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("attachment.bin");
        fs::write(&path, vec![0x5a; 64 * 1024 * 1024]).unwrap();
        let mmap = unsafe { memmap2::Mmap::map(&File::open(&path).unwrap()).unwrap() };
        let rounds = 5;
        let stream_start = Instant::now();
        let mut stream_digest = blake3::Hash::from([0; 32]);
        for _ in 0..rounds {
            let mut file = File::open(&path).unwrap();
            let mut hasher = blake3::Hasher::new();
            let mut buffer = [0_u8; BUFFER_SIZE];
            loop {
                let read = file.read(&mut buffer).unwrap();
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
            }
            stream_digest = hasher.finalize();
        }
        let stream = stream_start.elapsed();
        let mmap_start = Instant::now();
        let mut mmap_digest = blake3::Hash::from([0; 32]);
        for _ in 0..rounds {
            let mut hasher = blake3::Hasher::new();
            hasher.update_rayon(&mmap);
            mmap_digest = hasher.finalize();
        }
        let mmap_elapsed = mmap_start.elapsed();
        assert_eq!(stream_digest, mmap_digest);
        eprintln!("64 MiB x {rounds}: stream={stream:?}, mmap_parallel={mmap_elapsed:?}");
    }
}
