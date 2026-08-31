use std::{
    env, fs,
    io::{self, BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::{
    Result,
    credentials::{CredentialId, CredentialStore, KeyringStore},
    gmail::{self, Authorization, ImportedClient},
    repository::{NewAccount, OAuthClientProfile, Repository, validate_alias},
};

pub fn run(repository: &Repository, reauthorize: Option<&str>) -> Result<()> {
    run_with_progress(repository, reauthorize, &crate::progress::none)
}

pub fn run_with_progress(
    repository: &Repository,
    reauthorize: Option<&str>,
    progress: crate::progress::Reporter<'_>,
) -> Result<()> {
    repository.require_integrity_ready()?;
    if !io::stdin().is_terminal() {
        return Err(io::Error::other("connect requires an interactive terminal").into());
    }
    let store = KeyringStore::new(repository.id());
    progress(crate::progress::Event::Suspend);
    match reauthorize {
        Some(alias) => reauthorize_account(
            repository,
            &store,
            alias,
            |id, secret| gmail::authorize_with_progress(id, secret, progress),
            progress,
        ),
        None => connect_account(
            repository,
            &store,
            |id, secret| gmail::authorize_with_progress(id, secret, progress),
            progress,
        ),
    }
}

fn connect_account(
    repository: &Repository,
    store: &dyn CredentialStore,
    authorize: impl FnOnce(&str, &str) -> Result<Authorization>,
    progress: crate::progress::Reporter<'_>,
) -> Result<()> {
    let alias = prompt_account_alias(repository)?;
    let (profile, secret) = select_profile(repository, store, None)?;
    crate::progress::phase(progress, "Waiting for Gmail authorization");
    let authorization = authorize(&profile.client_id, &secret)?;
    crate::progress::phase(progress, "Saving account credentials");
    commit_account(
        repository,
        store,
        Uuid::new_v4(),
        &alias,
        &profile,
        authorization,
        progress,
    )
}

fn commit_account(
    repository: &Repository,
    store: &dyn CredentialStore,
    id: Uuid,
    alias: &str,
    profile: &OAuthClientProfile,
    authorization: Authorization,
    progress: crate::progress::Reporter<'_>,
) -> Result<()> {
    if repository.accounts()?.iter().any(|account| {
        account.provider == "gmail"
            && account
                .provider_identity
                .as_ref()
                .is_some_and(|identity| identity.eq_ignore_ascii_case(&authorization.email))
    }) {
        return Err(io::Error::other(format!(
            "provider mailbox identity is already configured: {}",
            authorization.email
        ))
        .into());
    }
    let credential = CredentialId::AccountRefresh(id);
    store.set(credential, &authorization.refresh_token)?;
    match repository.create_account_with_id(
        id,
        NewAccount {
            alias,
            provider: "gmail",
            provider_identity: Some(&authorization.email),
            credential_profile: Some(&profile.alias),
        },
    ) {
        Ok(account) => {
            progress(crate::progress::Event::Suspend);
            println!(
                "Connected {} as {} ({})",
                authorization.email, account.alias, account.id
            );
            Ok(())
        }
        Err(error) => match store.delete(credential) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(io::Error::other(format!(
                "{error}; failed to clean up refresh token: {cleanup}"
            ))
            .into()),
        },
    }
}

fn reauthorize_account(
    repository: &Repository,
    store: &dyn CredentialStore,
    alias: &str,
    authorize: impl FnOnce(&str, &str) -> Result<Authorization>,
    progress: crate::progress::Reporter<'_>,
) -> Result<()> {
    let account = repository.account_by_alias(alias)?;
    if account.provider != "gmail" {
        return Err(io::Error::other(format!("account {alias} is not a Gmail account")).into());
    }
    let profile_alias = account
        .credential_profile
        .as_deref()
        .ok_or_else(|| io::Error::other("account has no OAuth client profile"))?;
    let (profile, secret) = select_profile(repository, store, Some(profile_alias))?;
    crate::progress::phase(progress, "Waiting for Gmail authorization");
    let authorization = authorize(&profile.client_id, &secret)?;
    if !account
        .provider_identity
        .as_ref()
        .is_some_and(|identity| identity.eq_ignore_ascii_case(&authorization.email))
    {
        return Err(io::Error::other(format!(
            "authorized mailbox {} does not match account {alias}",
            authorization.email
        ))
        .into());
    }
    let _lock = repository.account_lock(account.id)?;
    if repository.account_by_alias(alias)? != account {
        return Err(
            io::Error::other(format!("account {alias} changed during authorization")).into(),
        );
    }
    crate::progress::phase(progress, "Saving account credentials");
    store.set(
        CredentialId::AccountRefresh(account.id),
        &authorization.refresh_token,
    )?;
    progress(crate::progress::Event::Suspend);
    println!("Reauthorized {alias} ({})", authorization.email);
    Ok(())
}

fn select_profile(
    repository: &Repository,
    store: &dyn CredentialStore,
    required_alias: Option<&str>,
) -> Result<(OAuthClientProfile, String)> {
    let profiles = repository.config()?.oauth_clients;
    let alias = match required_alias {
        Some(alias) => alias.to_owned(),
        None => prompt_profile_alias(&profiles)?,
    };
    let profile = profiles.into_iter().find(|profile| profile.alias == alias);
    match profile {
        Some(profile) => {
            if profile.provider != "google" {
                return Err(
                    io::Error::other("OAuth client profile is not a Google profile").into(),
                );
            }
            let id = CredentialId::OAuthClient(profile.id);
            let secret = match store.get(id)? {
                Some(secret) => secret,
                None => {
                    let imported = prompt_client(repository, Some(&profile.client_id))?;
                    store.set(id, &imported.client_secret)?;
                    imported.client_secret
                }
            };
            Ok((profile, secret))
        }
        None if required_alias.is_none() => {
            let imported = prompt_client(repository, None)?;
            let profile = OAuthClientProfile {
                id: Uuid::new_v4(),
                alias,
                provider: "google".to_owned(),
                client_id: imported.client_id,
            };
            let id = CredentialId::OAuthClient(profile.id);
            store.set(id, &imported.client_secret)?;
            if let Err(error) = repository.add_oauth_profile(profile.clone()) {
                return match store.delete(id) {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(io::Error::other(format!(
                        "{error}; failed to clean up OAuth client secret: {cleanup}"
                    ))
                    .into()),
                };
            }
            Ok((profile, imported.client_secret))
        }
        None => Err(io::Error::other(format!("unknown OAuth client profile: {alias}")).into()),
    }
}

fn prompt_account_alias(repository: &Repository) -> Result<String> {
    let default = default_account_alias(repository)?;
    loop {
        let alias = prompt(
            "Account alias",
            "Choose a short local name used with --account.\nExamples: personal, work, group. Use 1-32 lowercase letters/digits with internal '-' or '_'.",
            default,
        )?;
        match validate_alias(&alias) {
            Err(error) => eprintln!("Invalid account alias: {error}. Try again."),
            Ok(())
                if repository
                    .accounts()?
                    .iter()
                    .any(|account| account.alias == alias) =>
            {
                eprintln!("Account alias '{alias}' already exists. Choose another.")
            }
            Ok(()) => return Ok(alias),
        }
    }
}

fn default_account_alias(repository: &Repository) -> Result<Option<&'static str>> {
    let accounts = repository.accounts()?;
    Ok(["personal", "work", "group"]
        .into_iter()
        .find(|candidate| !accounts.iter().any(|account| account.alias == *candidate)))
}

fn prompt_profile_alias(profiles: &[OAuthClientProfile]) -> Result<String> {
    let aliases = profiles
        .iter()
        .map(|profile| profile.alias.as_str())
        .collect::<Vec<_>>();
    let help = if aliases.is_empty() {
        "Name the reusable Google Desktop OAuth credentials. A new profile imports a client JSON."
            .to_owned()
    } else {
        format!(
            "Reuse a Google Desktop OAuth profile across Gmail accounts.\nExisting profiles: {}. Enter one to reuse it, or a new alias to import another client JSON.",
            aliases.join(", ")
        )
    };
    let default = default_profile_alias(profiles);
    loop {
        let alias = prompt("OAuth client profile alias", &help, default)?;
        match validate_alias(&alias) {
            Ok(()) => return Ok(alias),
            Err(error) => eprintln!("Invalid OAuth client profile alias: {error}. Try again."),
        }
    }
}

fn default_profile_alias(profiles: &[OAuthClientProfile]) -> Option<&str> {
    match profiles {
        [] => Some("google"),
        [profile] => Some(&profile.alias),
        _ => None,
    }
}

fn prompt_client(
    repository: &Repository,
    expected_client_id: Option<&str>,
) -> Result<ImportedClient> {
    const HELP: &str = "Download a Desktop app client JSON: Google Cloud Console > Google Auth Platform > Clients > Create client > Desktop app > Download JSON.\nGuide: https://developers.google.com/workspace/gmail/api/quickstart/python\nExample: ~/Downloads/client_secret_123456.apps.googleusercontent.com.json\nKeep this credential file outside the bit-mail repository.";
    loop {
        let path = prompt("Desktop OAuth client JSON path", HELP, None)?;
        match import_client(repository, &path) {
            Ok(imported)
                if expected_client_id.is_none_or(|expected| imported.client_id == expected) =>
            {
                return Ok(imported);
            }
            Ok(_) => eprintln!(
                "Cannot use that OAuth client JSON: its client ID does not match the saved profile. Try again."
            ),
            Err(error) => {
                eprintln!("Cannot use that OAuth client JSON: {error}. Try again.")
            }
        }
    }
}

fn import_client(repository: &Repository, path: &str) -> Result<ImportedClient> {
    let home = env::var_os("HOME").map(PathBuf::from);
    let path = expand_tilde(path, home.as_deref())?;
    let path = fs::canonicalize(&path)
        .map_err(|error| io::Error::other(format!("cannot open {}: {error}", path.display())))?;
    if path.starts_with(fs::canonicalize(repository.root())?) {
        return Err(io::Error::other(
            "the credential file is inside the bit-mail repository; move it outside first",
        )
        .into());
    }
    gmail::parse_desktop_client(&fs::read_to_string(path)?)
}

fn expand_tilde(path: &str, home: Option<&Path>) -> Result<PathBuf> {
    let Some(relative) = path.strip_prefix("~/") else {
        return Ok(PathBuf::from(path));
    };
    let home = home.ok_or_else(|| io::Error::other("cannot expand '~': HOME is not set"))?;
    Ok(home.join(relative))
}

fn prompt(label: &str, help: &str, default: Option<&str>) -> Result<String> {
    prompt_from(
        &mut io::stdin().lock(),
        &mut io::stderr().lock(),
        label,
        help,
        default,
    )
}

fn prompt_from(
    input: &mut impl BufRead,
    output: &mut impl Write,
    label: &str,
    help: &str,
    default: Option<&str>,
) -> Result<String> {
    writeln!(output, "{help}")?;
    loop {
        write!(output, "{label}")?;
        if let Some(default) = default {
            write!(output, " [{default}]")?;
        }
        write!(output, ": ")?;
        output.flush()?;
        let mut value = String::new();
        if input.read_line(&mut value)? == 0 {
            return Err(io::Error::other("interactive input closed").into());
        }
        let value = value.trim();
        if !value.is_empty() {
            return Ok(value.to_owned());
        }
        if let Some(default) = default {
            return Ok(default.to_owned());
        }
        writeln!(output, "{label} is required. Try again.")?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, io::Cursor, sync::Mutex};

    use crate::repository::{
        AccountConfig, GitIgnorePolicy, RemoveOptions, UnavailableCredentialRevoker,
    };

    #[derive(Default)]
    struct MemoryStore(Mutex<HashMap<CredentialId, String>>);
    impl CredentialStore for MemoryStore {
        fn get(&self, id: CredentialId) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().get(&id).cloned())
        }
        fn set(&self, id: CredentialId, secret: &str) -> Result<()> {
            self.0.lock().unwrap().insert(id, secret.to_owned());
            Ok(())
        }
        fn delete(&self, id: CredentialId) -> Result<()> {
            self.0.lock().unwrap().remove(&id);
            Ok(())
        }
    }

    #[test]
    fn guided_prompts_accept_safe_defaults_and_retry_missing_values() {
        let mut output = Vec::new();
        assert_eq!(
            prompt_from(
                &mut Cursor::new(b"\n"),
                &mut output,
                "Account alias",
                "Choose an alias.",
                Some("personal")
            )
            .unwrap(),
            "personal"
        );
        assert!(String::from_utf8(output).unwrap().contains("[personal]"));

        let mut output = Vec::new();
        assert_eq!(
            prompt_from(
                &mut Cursor::new(b"\nwork\n"),
                &mut output,
                "Account alias",
                "Choose an alias.",
                None
            )
            .unwrap(),
            "work"
        );
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("Account alias is required")
        );
    }

    #[test]
    fn connect_defaults_only_when_the_choice_is_unambiguous() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        assert_eq!(
            default_account_alias(&repository).unwrap(),
            Some("personal")
        );

        for (alias, next) in [
            ("personal", Some("work")),
            ("work", Some("group")),
            ("group", None),
        ] {
            repository
                .create_account(NewAccount {
                    alias,
                    provider: "gmail",
                    provider_identity: None,
                    credential_profile: None,
                })
                .unwrap();
            assert_eq!(default_account_alias(&repository).unwrap(), next);
        }

        assert_eq!(default_profile_alias(&[]), Some("google"));
        let profile = OAuthClientProfile {
            id: Uuid::new_v4(),
            alias: "company".into(),
            provider: "google".into(),
            client_id: "id".into(),
        };
        assert_eq!(
            default_profile_alias(std::slice::from_ref(&profile)),
            Some("company")
        );
        assert_eq!(default_profile_alias(&[profile.clone(), profile]), None);
    }

    #[test]
    fn oauth_json_supports_home_paths_but_not_repository_files() {
        assert_eq!(
            expand_tilde("~/Downloads/client.json", Some(Path::new("/home/person"))).unwrap(),
            Path::new("/home/person/Downloads/client.json")
        );

        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let json = r#"{"installed":{"client_id":"id","client_secret":"secret","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token"}}"#;
        let inside = directory.path().join("client.json");
        fs::write(&inside, json).unwrap();
        assert!(
            import_client(&repository, inside.to_str().unwrap())
                .err()
                .unwrap()
                .to_string()
                .contains("inside the bit-mail repository")
        );

        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("client.json");
        fs::write(&path, json).unwrap();
        assert_eq!(
            import_client(&repository, path.to_str().unwrap())
                .unwrap()
                .client_id,
            "id"
        );
    }

    fn account_fixture(
        provider: &str,
    ) -> (tempfile::TempDir, Repository, AccountConfig, MemoryStore) {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let profile = OAuthClientProfile {
            id: Uuid::new_v4(),
            alias: "google".into(),
            provider: "google".into(),
            client_id: "id".into(),
        };
        repository.add_oauth_profile(profile.clone()).unwrap();
        let account = repository
            .create_account(NewAccount {
                alias: "personal",
                provider,
                provider_identity: Some("one@example.com"),
                credential_profile: Some("google"),
            })
            .unwrap();
        let store = MemoryStore::default();
        store
            .set(CredentialId::OAuthClient(profile.id), "client-secret")
            .unwrap();
        store
            .set(CredentialId::AccountRefresh(account.id), "old")
            .unwrap();
        (directory, repository, account, store)
    }

    #[test]
    fn reauthorization_only_replaces_the_token_for_the_same_mailbox() {
        let (_directory, repository, account, store) = account_fixture("gmail");
        assert!(
            reauthorize_account(
                &repository,
                &store,
                "personal",
                |_, _| Ok(Authorization {
                    email: "two@example.com".into(),
                    refresh_token: "new".into()
                }),
                &crate::progress::none
            )
            .is_err()
        );
        assert_eq!(
            store
                .get(CredentialId::AccountRefresh(account.id))
                .unwrap()
                .as_deref(),
            Some("old")
        );
        reauthorize_account(
            &repository,
            &store,
            "personal",
            |_, _| {
                Ok(Authorization {
                    email: "ONE@EXAMPLE.COM".into(),
                    refresh_token: "new".into(),
                })
            },
            &crate::progress::none,
        )
        .unwrap();
        assert_eq!(
            store
                .get(CredentialId::AccountRefresh(account.id))
                .unwrap()
                .as_deref(),
            Some("new")
        );
    }

    #[test]
    fn reauthorization_does_not_restore_credentials_for_a_removed_account() {
        let (_directory, repository, account, store) = account_fixture("gmail");

        assert!(
            reauthorize_account(
                &repository,
                &store,
                "personal",
                |_, _| {
                    repository.remove_account(
                        "personal",
                        RemoveOptions {
                            discard_local_data: false,
                            keep_credentials: true,
                            revoke_credentials: false,
                        },
                        &UnavailableCredentialRevoker,
                    )?;
                    Ok(Authorization {
                        email: "one@example.com".into(),
                        refresh_token: "new".into(),
                    })
                },
                &crate::progress::none
            )
            .is_err()
        );
        assert_eq!(
            store
                .get(CredentialId::AccountRefresh(account.id))
                .unwrap()
                .as_deref(),
            Some("old")
        );
        assert!(repository.accounts().unwrap().is_empty());
    }

    #[test]
    fn reauthorization_rejects_non_gmail_accounts_before_oauth() {
        let (_directory, repository, _account, store) = account_fixture("imap");

        let error = reauthorize_account(
            &repository,
            &store,
            "personal",
            |_, _| panic!("OAuth must not start for a non-Gmail account"),
            &crate::progress::none,
        )
        .unwrap_err();

        assert!(error.to_string().contains("not a Gmail account"));
    }

    #[test]
    fn one_profile_keeps_account_authorizations_separate() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            Repository::initialize(directory.path(), crate::repository::GitIgnorePolicy::Never)
                .unwrap();
        let profile = OAuthClientProfile {
            id: Uuid::new_v4(),
            alias: "google".into(),
            provider: "google".into(),
            client_id: "public-id".into(),
        };
        repository.add_oauth_profile(profile.clone()).unwrap();
        let store = MemoryStore::default();
        for (alias, email, token) in [
            ("personal", "one@example.com", "refresh-one"),
            ("work", "two@example.com", "refresh-two"),
        ] {
            commit_account(
                &repository,
                &store,
                Uuid::new_v4(),
                alias,
                &profile,
                Authorization {
                    email: email.into(),
                    refresh_token: token.into(),
                },
                &crate::progress::none,
            )
            .unwrap();
        }
        let accounts = repository.accounts().unwrap();
        assert_eq!(accounts.len(), 2);
        assert_ne!(
            store
                .get(CredentialId::AccountRefresh(accounts[0].id))
                .unwrap(),
            store
                .get(CredentialId::AccountRefresh(accounts[1].id))
                .unwrap()
        );
        let config =
            std::fs::read_to_string(directory.path().join(".bit-mail/config.toml")).unwrap();
        assert!(!config.contains("refresh-one"));
        assert!(!config.contains("refresh-two"));
    }

    #[test]
    fn failed_account_publication_removes_the_refresh_token() {
        let directory = tempfile::tempdir().unwrap();
        let repository =
            Repository::initialize(directory.path(), crate::repository::GitIgnorePolicy::Never)
                .unwrap();
        let profile = OAuthClientProfile {
            id: Uuid::new_v4(),
            alias: "google".into(),
            provider: "google".into(),
            client_id: "public-id".into(),
        };
        let store = MemoryStore::default();
        let id = Uuid::new_v4();
        std::fs::create_dir(repository.data_dir(id)).unwrap();
        assert!(
            commit_account(
                &repository,
                &store,
                id,
                "personal",
                &profile,
                Authorization {
                    email: "one@example.com".into(),
                    refresh_token: "refresh".into(),
                },
                &crate::progress::none,
            )
            .is_err()
        );
        assert_eq!(store.get(CredentialId::AccountRefresh(id)).unwrap(), None);
        assert!(repository.accounts().unwrap().is_empty());
    }
}
