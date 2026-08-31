use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

use serde::Serialize;

use crate::{
    Result,
    credentials::{CredentialId, CredentialStore},
    repository::{AccountConfig, Repository},
    storage::CanonicalStore,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Serialize)]
pub struct Remediation {
    pub command: String,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct IntegrityFinding {
    pub kind: &'static str,
    pub object: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<uuid::Uuid>,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub code: &'static str,
    pub status: Status,
    pub scope: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<uuid::Uuid>,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<IntegrityFinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<Remediation>,
}

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub schema_version: u32,
    pub status: Status,
    pub checks: Vec<Check>,
}

impl DoctorReport {
    pub fn failed(&self) -> bool {
        self.status == Status::Error
    }

    pub fn render(&self) -> String {
        let mut output = String::new();
        for check in &self.checks {
            let account = check
                .account_id
                .map(|id| format!(" account={id}"))
                .unwrap_or_default();
            output.push_str(&format!(
                "{:?}\t{}{}\t{}\n",
                check.status, check.code, account, check.message
            ));
            if let Some(remediation) = &check.remediation {
                output.push_str(&format!("  run: {}\n", remediation.command));
            }
            for finding in &check.findings {
                let id = finding.id.map(|id| format!(" id={id}")).unwrap_or_default();
                output.push_str(&format!(
                    "  finding: {} {}{}\n",
                    finding.kind, finding.object, id
                ));
            }
        }
        output
    }
}

pub fn repository_failure() -> DoctorReport {
    DoctorReport {
        schema_version: 1,
        status: Status::Error,
        checks: vec![Check {
            code: "repository.format",
            status: Status::Error,
            scope: "repository",
            account_id: None,
            message: "repository discovery, format, or configuration validation failed".into(),
            findings: Vec::new(),
            remediation: None,
        }],
    }
}

pub struct Options<'a> {
    pub account: Option<&'a str>,
    pub all_accounts: bool,
    pub full: bool,
    pub online: bool,
}

pub fn run(
    repository: &Repository,
    options: Options<'_>,
    credentials: &dyn CredentialStore,
    online_probe: impl FnMut(&AccountConfig) -> Result<()>,
) -> DoctorReport {
    run_with_progress(
        repository,
        options,
        credentials,
        online_probe,
        &crate::progress::none,
    )
}

pub fn run_with_progress(
    repository: &Repository,
    options: Options<'_>,
    credentials: &dyn CredentialStore,
    mut online_probe: impl FnMut(&AccountConfig) -> Result<()>,
    progress: crate::progress::Reporter<'_>,
) -> DoctorReport {
    crate::progress::phase(progress, "Checking repository health");
    let mut checks = Vec::new();
    ok(
        &mut checks,
        "repository.format",
        "repository",
        None,
        "repository and configuration schemas are supported",
    );
    permission_checks(repository, &mut checks);
    git_checks(repository, &mut checks);
    lock_checks(repository, &mut checks);

    match crate::integrity::validate_repository(repository) {
        Ok(found) if found.is_empty() => ok(
            &mut checks,
            "runtime.integrity",
            "repository",
            None,
            "runtime templates and repository state are integrity-valid",
        ),
        Ok(found) => integrity_problem(
            &mut checks,
            "runtime.integrity",
            "repository",
            None,
            "runtime templates or repository state failed integrity validation",
            found,
            None,
        ),
        Err(_) => problem(
            &mut checks,
            "runtime.integrity",
            Status::Error,
            "repository",
            None,
            "runtime integrity could not be validated",
            None,
        ),
    }
    if repository.runtime_assets_version() == Some(env!("CARGO_PKG_VERSION")) {
        ok(
            &mut checks,
            "runtime.version",
            "repository",
            None,
            "runtime assets match the binary version",
        );
    } else {
        problem(
            &mut checks,
            "runtime.version",
            Status::Warning,
            "repository",
            None,
            "runtime assets do not match the binary version",
            Some("bit-mail accounts".into()),
        );
    }

    let all = match repository.accounts() {
        Ok(accounts) => accounts,
        Err(_) => {
            problem(
                &mut checks,
                "account.config",
                Status::Error,
                "repository",
                None,
                "account configuration could not be read consistently",
                None,
            );
            Vec::new()
        }
    };
    let accounts = if options.all_accounts {
        all.clone()
    } else if let Some(alias) = options.account {
        match all.iter().find(|account| account.alias == alias) {
            Some(account) => vec![account.clone()],
            None => {
                problem(
                    &mut checks,
                    "account.selection",
                    Status::Error,
                    "repository",
                    None,
                    "selected account does not exist",
                    Some("bit-mail accounts".into()),
                );
                Vec::new()
            }
        }
    } else {
        match all.as_slice() {
            [account] => vec![account.clone()],
            [] => {
                problem(
                    &mut checks,
                    "account.selection",
                    Status::Warning,
                    "repository",
                    None,
                    "no accounts are configured",
                    Some("bit-mail connect".into()),
                );
                Vec::new()
            }
            _ => {
                problem(
                    &mut checks,
                    "account.selection",
                    Status::Warning,
                    "repository",
                    None,
                    "multiple accounts require --account or --all-accounts",
                    Some("bit-mail doctor --all-accounts".into()),
                );
                Vec::new()
            }
        }
    };

    if options.full {
        crate::progress::phase(progress, "Validating full repository integrity");
        match crate::integrity::validate_full(repository) {
            Ok(value) if value.mismatches.is_empty() => ok(
                &mut checks,
                "integrity.full",
                "repository",
                None,
                "all integrity-covered bytes are valid",
            ),
            Ok(value) => integrity_problem(
                &mut checks,
                "integrity.full",
                "repository",
                None,
                "full integrity validation found mismatches",
                value.mismatches,
                None,
            ),
            Err(_) => problem(
                &mut checks,
                "integrity.full",
                Status::Error,
                "repository",
                None,
                "full integrity validation could not complete",
                None,
            ),
        }
    }

    let config = repository.config().ok();
    for account in accounts {
        crate::progress::phase(progress, format!("Checking account {}", account.alias));
        let id = Some(account.id);
        if account.provider != "gmail"
            || account.provider_identity.is_none()
            || account.credential_profile.is_none()
        {
            problem(
                &mut checks,
                "account.config",
                Status::Error,
                "account",
                id,
                "account provider or credential profile is inconsistent",
                Some(format!("bit-mail connect --reauthorize {}", account.alias)),
            );
        } else {
            ok(
                &mut checks,
                "account.config",
                "account",
                id,
                "account configuration is consistent",
            );
        }

        let profile = config.as_ref().and_then(|config| {
            config.oauth_clients.iter().find(|profile| {
                Some(profile.alias.as_str()) == account.credential_profile.as_deref()
            })
        });
        let credential_health = profile.ok_or(()).and_then(|profile| {
            let client = credentials
                .get(CredentialId::OAuthClient(profile.id))
                .map_err(|_| ())?;
            let refresh = credentials
                .get(CredentialId::AccountRefresh(account.id))
                .map_err(|_| ())?;
            (client.is_some() && refresh.is_some())
                .then_some(())
                .ok_or(())
        });
        if credential_health.is_ok() {
            ok(
                &mut checks,
                "credentials.lookup",
                "account",
                id,
                "credential store entries are available",
            );
        } else {
            problem(
                &mut checks,
                "credentials.lookup",
                Status::Error,
                "account",
                id,
                "credential store is unavailable or required entries are missing",
                Some(format!("bit-mail connect --reauthorize {}", account.alias)),
            );
        }

        check_result(
            &mut checks,
            "provider.state",
            id,
            crate::pull::validate_provider_state(repository, account.id),
            "provider cursor state is structurally valid",
            "provider cursor state is invalid",
            Some("bit-mail cache rebuild".into()),
        );
        check_result(
            &mut checks,
            "index.sqlite",
            id,
            CanonicalStore::new(repository, &account).and_then(|store| store.validate_index()),
            "structural index is valid",
            "structural index is missing or invalid",
            Some("bit-mail index rebuild".into()),
        );
        match crate::recovery::gc(repository, &account, true) {
            Ok(report) if report.threads == 0 && report.messages.is_empty() => ok(
                &mut checks,
                "cache.reachability",
                "account",
                id,
                "provider-derived cache is reachable",
            ),
            Ok(_) => problem(
                &mut checks,
                "cache.reachability",
                Status::Warning,
                "account",
                id,
                "unreachable provider-derived cache is present",
                Some("bit-mail gc".into()),
            ),
            Err(_) => problem(
                &mut checks,
                "cache.reachability",
                Status::Error,
                "account",
                id,
                "cache reachability could not be diagnosed",
                None,
            ),
        }
        if !options.full {
            match crate::integrity::validate_account(repository, account.id) {
                Ok(found) if found.is_empty() => ok(
                    &mut checks,
                    "integrity.account",
                    "account",
                    id,
                    "account canonical state is integrity-valid",
                ),
                Ok(found) => integrity_problem(
                    &mut checks,
                    "integrity.account",
                    "account",
                    id,
                    "account canonical state failed integrity validation",
                    found.clone(),
                    found
                        .iter()
                        .find_map(|mismatch| message_id(&mismatch.path, account.id))
                        .map(|message| format!("bit-mail repair {message}")),
                ),
                Err(_) => problem(
                    &mut checks,
                    "integrity.account",
                    Status::Error,
                    "account",
                    id,
                    "account canonical state could not be validated",
                    None,
                ),
            }
        }
        if options.online {
            crate::progress::phase(progress, format!("Checking Gmail for {}", account.alias));
            check_result(
                &mut checks,
                "gmail.authorization",
                id,
                online_probe(&account),
                "Gmail authorization is valid",
                "Gmail authorization failed",
                Some(format!("bit-mail connect --reauthorize {}", account.alias)),
            );
        }
    }

    checks.sort_by(|a, b| (a.scope, a.account_id, a.code).cmp(&(b.scope, b.account_id, b.code)));
    let status = if checks.iter().any(|check| check.status == Status::Error) {
        Status::Error
    } else if checks.iter().any(|check| check.status == Status::Warning) {
        Status::Warning
    } else {
        Status::Ok
    };
    DoctorReport {
        schema_version: 1,
        status,
        checks,
    }
}

fn message_id(path: &str, account_id: uuid::Uuid) -> Option<uuid::Uuid> {
    path.split('/')
        .filter_map(|part| part.trim_end_matches(".json").parse().ok())
        .find(|id| *id != account_id)
}

fn check_result(
    checks: &mut Vec<Check>,
    code: &'static str,
    id: Option<uuid::Uuid>,
    result: Result<()>,
    good: &str,
    bad: &str,
    remediation: Option<String>,
) {
    if result.is_ok() {
        ok(checks, code, "account", id, good);
    } else {
        problem(checks, code, Status::Error, "account", id, bad, remediation);
    }
}

fn ok(
    checks: &mut Vec<Check>,
    code: &'static str,
    scope: &'static str,
    id: Option<uuid::Uuid>,
    message: &str,
) {
    problem(checks, code, Status::Ok, scope, id, message, None)
}

fn problem(
    checks: &mut Vec<Check>,
    code: &'static str,
    status: Status,
    scope: &'static str,
    account_id: Option<uuid::Uuid>,
    message: &str,
    command: Option<String>,
) {
    checks.push(Check {
        code,
        status,
        scope,
        account_id,
        message: message.into(),
        findings: Vec::new(),
        remediation: command.map(|command| Remediation { command }),
    });
}

fn integrity_problem(
    checks: &mut Vec<Check>,
    code: &'static str,
    scope: &'static str,
    account_id: Option<uuid::Uuid>,
    message: &str,
    mismatches: Vec<crate::integrity::IntegrityMismatch>,
    command: Option<String>,
) {
    problem(
        checks,
        code,
        Status::Error,
        scope,
        account_id,
        message,
        command,
    );
    let findings = &mut checks
        .last_mut()
        .expect("diagnostic was just added")
        .findings;
    findings.extend(
        mismatches
            .iter()
            .map(|mismatch| finding(mismatch, account_id)),
    );
    findings.sort();
    findings.dedup();
}

fn finding(
    mismatch: &crate::integrity::IntegrityMismatch,
    account_hint: Option<uuid::Uuid>,
) -> IntegrityFinding {
    let ids = mismatch
        .path
        .split('/')
        .filter_map(|part| {
            part.trim_end_matches(".json")
                .trim_end_matches(".md")
                .parse()
                .ok()
        })
        .collect::<Vec<uuid::Uuid>>();
    let account_id = account_hint.or_else(|| {
        (mismatch.path.starts_with(".bit-mail/accounts/")
            || mismatch.path.starts_with("data/")
            || mismatch.path.starts_with("knowledge/accounts/"))
        .then(|| ids.first().copied())
        .flatten()
    });
    let (object, id) = if mismatch.path == "AGENTS.md"
        || mismatch.path.starts_with("skills/")
        || mismatch.path.starts_with("templates/")
    {
        ("runtime_asset", None)
    } else if mismatch.path.starts_with("knowledge/") {
        (
            "knowledge",
            ids.iter().copied().find(|id| Some(*id) != account_id),
        )
    } else if let Some(id) = ids.iter().copied().find(|id| Some(*id) != account_id) {
        ("message", Some(id))
    } else if account_id.is_some() {
        ("account", account_id)
    } else {
        ("repository", None)
    };
    IntegrityFinding {
        kind: mismatch.kind,
        object,
        id,
    }
}

fn permission_checks(repository: &Repository, checks: &mut Vec<Check>) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut unsafe_paths = 0usize;
        for root in [".bit-mail", "data", "knowledge"].map(|path| repository.root().join(path)) {
            walk(&root, &mut |path| {
                if fs::symlink_metadata(path)
                    .ok()
                    .is_some_and(|metadata| metadata.permissions().mode() & 0o077 != 0)
                {
                    unsafe_paths += 1;
                }
            });
        }
        if unsafe_paths > 0 {
            problem(
                checks,
                "permissions.private",
                Status::Warning,
                "repository",
                None,
                &format!("{unsafe_paths} private path(s) have unsafe mode bits"),
                Some("chmod -R go-rwx -- .bit-mail data knowledge".into()),
            );
        } else {
            ok(
                checks,
                "permissions.private",
                "repository",
                None,
                "private runtime paths have restrictive mode bits",
            );
        }
        problem(
            checks,
            "permissions.acl",
            Status::Warning,
            "repository",
            None,
            "filesystem ACL restrictions could not be verified from mode bits",
            None,
        );
    }
    #[cfg(not(unix))]
    problem(
        checks,
        "permissions.private",
        Status::Warning,
        "repository",
        None,
        "platform permission semantics could not be verified",
        None,
    );
}

fn walk(path: &Path, inspect: &mut impl FnMut(&Path)) {
    inspect(path);
    if let Ok(entries) = fs::read_dir(path) {
        let mut entries = entries.flatten().collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                walk(&path, inspect);
            } else {
                inspect(&path);
            }
        }
    }
}

fn git_checks(repository: &Repository, checks: &mut Vec<Check>) {
    let Some(root) = repository.root().ancestors().find(|path| {
        let marker = path.join(".git");
        marker.is_file() || marker.join("HEAD").is_file()
    }) else {
        ok(
            checks,
            "git.private-paths",
            "repository",
            None,
            "runtime repository is not inside Git",
        );
        return;
    };
    let relative = repository
        .root()
        .strip_prefix(root)
        .unwrap_or(Path::new(""));
    let mut exposed = false;
    for name in [".bit-mail", "data", "knowledge"] {
        let path = relative.join(name);
        let tracked = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["ls-files", "--error-unmatch", "--"])
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        let ignored = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["check-ignore", "--quiet", "--"])
            .arg(&path)
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        exposed |= tracked || !ignored;
    }
    if exposed {
        problem(
            checks,
            "git.private-paths",
            Status::Warning,
            "repository",
            None,
            "private runtime paths are tracked or not fully ignored",
            Some("git status --ignored".into()),
        );
    } else {
        ok(
            checks,
            "git.private-paths",
            "repository",
            None,
            "private runtime paths are ignored and untracked",
        );
    }
}

fn lock_checks(repository: &Repository, checks: &mut Vec<Check>) {
    let mut locks = Vec::new();
    walk(&repository.root().join(".bit-mail/locks"), &mut |path| {
        if path.extension().and_then(|v| v.to_str()) == Some("lock") {
            locks.push(path.to_path_buf());
        }
    });
    if locks.is_empty() {
        ok(
            checks,
            "locks.state",
            "repository",
            None,
            "no mutation locks are present",
        );
    } else {
        let mut stale = Vec::new();
        let mut active = Vec::new();
        for path in &locks {
            let pid = fs::read_to_string(path).ok().and_then(|value| {
                value
                    .split_whitespace()
                    .find_map(|field| field.strip_prefix("pid="))?
                    .parse::<u32>()
                    .ok()
            });
            if let Some(pid) = pid.filter(|pid| process_alive(*pid)) {
                active.push(pid);
            } else {
                stale.push(path);
            }
        }
        active.sort_unstable();
        active.dedup();
        let (status, message, remediation) = if !stale.is_empty() {
            (
                Status::Warning,
                format!(
                    "{} stale/unreadable and {} active mutation lock(s) detected",
                    stale.len(),
                    active.len()
                ),
                stale
                    .iter()
                    .find_map(|path| managed_lock_remediation(repository, path)),
            )
        } else {
            (
                Status::Ok,
                format!("active mutation lock holder PID(s): {active:?}"),
                None,
            )
        };
        problem(
            checks,
            "locks.state",
            status,
            "repository",
            None,
            &message,
            remediation,
        );
    }
}

fn managed_lock_remediation(repository: &Repository, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(repository.root()).ok()?;
    for name in ["knowledge.lock", "account-lifecycle.lock"] {
        if relative == Path::new(".bit-mail/locks").join(name) {
            return Some(format!(
                "rm -- '.bit-mail/locks/{name}' # only after confirming its recorded process is absent"
            ));
        }
    }
    let account_id = relative
        .strip_prefix(".bit-mail/locks/accounts")
        .ok()?
        .file_name()?
        .to_str()?
        .strip_suffix(".lock")?
        .parse::<uuid::Uuid>()
        .ok()?;
    (relative.components().count() == 4).then(|| {
        format!(
            "rm -- '.bit-mail/locks/accounts/{account_id}.lock' # only after confirming its recorded process is absent"
        )
    })
}

fn process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    return Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    #[cfg(not(unix))]
    return true;
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Mutex};

    use super::*;
    use crate::repository::{GitIgnorePolicy, NewAccount, OAuthClientProfile};

    #[derive(Default)]
    struct MemoryStore(Mutex<HashMap<CredentialId, String>>);

    impl CredentialStore for MemoryStore {
        fn get(&self, id: CredentialId) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(&id).cloned())
        }
        fn set(&self, id: CredentialId, secret: &str) -> Result<()> {
            self.0.lock().unwrap().insert(id, secret.into());
            Ok(())
        }
        fn delete(&self, id: CredentialId) -> Result<()> {
            self.0.lock().unwrap().remove(&id);
            Ok(())
        }
    }

    #[test]
    fn doctor_is_offline_redacted_and_recommends_index_rebuild() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let profile_id = uuid::Uuid::new_v4();
        repository
            .add_oauth_profile(OAuthClientProfile {
                id: profile_id,
                alias: "google".into(),
                provider: "google".into(),
                client_id: "client".into(),
            })
            .unwrap();
        let account = repository
            .create_account(NewAccount {
                alias: "personal",
                provider: "gmail",
                provider_identity: Some("private@example.com"),
                credential_profile: Some("google"),
            })
            .unwrap();
        let store = MemoryStore::default();
        store
            .set(
                CredentialId::OAuthClient(profile_id),
                "sentinel-client-secret",
            )
            .unwrap();
        store
            .set(
                CredentialId::AccountRefresh(account.id),
                "sentinel-refresh-token",
            )
            .unwrap();
        let mut probes = 0;
        let report = run(
            &repository,
            Options {
                account: None,
                all_accounts: false,
                full: false,
                online: false,
            },
            &store,
            |_| {
                probes += 1;
                Ok(())
            },
        );
        let json = serde_json::to_string(&report).unwrap();
        assert_eq!(
            probes, 0,
            "doctor must remain offline unless explicitly requested"
        );
        for secret in ["sentinel", "private@example.com"] {
            assert!(!json.contains(secret), "diagnostics must redact {secret}");
        }
        assert!(json.contains("bit-mail index rebuild"));

        let online = run(
            &repository,
            Options {
                account: None,
                all_accounts: false,
                full: false,
                online: true,
            },
            &store,
            |_| {
                probes += 1;
                Ok(())
            },
        );
        assert_eq!(
            probes, 1,
            "--online must perform exactly one selected-account probe"
        );
        assert!(
            online
                .checks
                .iter()
                .any(|check| { check.code == "gmail.authorization" && check.status == Status::Ok })
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                repository.root().join("data"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            let unsafe_report = run(
                &repository,
                Options {
                    account: None,
                    all_accounts: false,
                    full: false,
                    online: false,
                },
                &store,
                |_| Ok(()),
            );
            assert!(unsafe_report.checks.iter().any(|check| {
                check.code == "permissions.private"
                    && check.status == Status::Warning
                    && check.remediation.as_ref().is_some_and(|value| {
                        value.command == "chmod -R go-rwx -- .bit-mail data knowledge"
                    })
            }));
            assert!(unsafe_report.checks.iter().any(|check| {
                check.code == "permissions.acl" && check.status == Status::Warning
            }));
        }
    }

    #[cfg(unix)]
    #[test]
    fn permission_diagnostics_are_deterministic_and_redact_private_paths() {
        use std::os::unix::fs::PermissionsExt;

        fn report(names: &[&str]) -> String {
            let directory = tempfile::tempdir().unwrap();
            let repository =
                Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
            for name in names {
                let path = repository.root().join("data").join(name);
                fs::write(&path, "private").unwrap();
                fs::set_permissions(path, fs::Permissions::from_mode(0o644)).unwrap();
            }
            serde_json::to_string(&run(
                &repository,
                Options {
                    account: None,
                    all_accounts: false,
                    full: false,
                    online: false,
                },
                &MemoryStore::default(),
                |_| Ok(()),
            ))
            .unwrap()
        }

        let names = [
            "private-subject-sentinel",
            "x'; echo unsafe; '",
            "private-line\nbreak",
        ];
        let forward = report(&names);
        let reverse = report(&names.into_iter().rev().collect::<Vec<_>>());
        assert_eq!(
            forward, reverse,
            "diagnostics must not depend on directory creation order"
        );
        for private in ["private-subject-sentinel", "echo unsafe", "private-line"] {
            assert!(
                !forward.contains(private),
                "diagnostics leaked private path text: {private}"
            );
        }
        assert!(forward.contains("3 private path(s) have unsafe mode bits"));
        assert!(forward.contains("chmod -R go-rwx -- .bit-mail data knowledge"));
    }

    #[cfg(unix)]
    #[test]
    fn stale_lock_commands_are_limited_to_managed_paths() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let locks = repository.root().join(".bit-mail/locks");
        let managed = locks.join("knowledge.lock");
        let unrecognized = locks.join("sentinel'; echo unsafe; '.lock");
        for path in [&managed, &unrecognized] {
            fs::write(path, "unreadable holder metadata").unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
        }

        let mut checks = Vec::new();
        lock_checks(&repository, &mut checks);
        let json = serde_json::to_string(&checks).unwrap();
        for private in ["sentinel", "echo unsafe"] {
            assert!(!json.contains(private), "lock diagnostics leaked {private}");
        }
        let check = checks
            .iter()
            .find(|check| check.code == "locks.state")
            .unwrap();
        assert_eq!(check.status, Status::Warning);
        assert_eq!(
            check
                .remediation
                .as_ref()
                .map(|value| value.command.as_str()),
            Some(
                "rm -- '.bit-mail/locks/knowledge.lock' # only after confirming its recorded process is absent"
            )
        );

        fs::remove_file(managed).unwrap();
        let mut checks = Vec::new();
        lock_checks(&repository, &mut checks);
        assert!(
            checks[0].remediation.is_none(),
            "unrecognized locks must not produce executable commands"
        );
    }
}
