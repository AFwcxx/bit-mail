use std::{
    fs,
    io::{self, IsTerminal, Write},
    path::Path,
};

use uuid::Uuid;

use crate::{
    Result,
    credentials::{CredentialId, CredentialStore, KeyringStore},
    gmail::{self, Authorization, ImportedClient},
    repository::{NewAccount, OAuthClientProfile, Repository, validate_alias},
};

pub fn run(repository: &Repository, reauthorize: Option<&str>) -> Result<()> {
    if !io::stdin().is_terminal() {
        return Err(io::Error::other("connect requires an interactive terminal").into());
    }
    let store = KeyringStore::new(repository.id());
    match reauthorize {
        Some(alias) => reauthorize_account(repository, &store, alias, gmail::authorize),
        None => connect_account(repository, &store, gmail::authorize),
    }
}

fn connect_account(
    repository: &Repository,
    store: &dyn CredentialStore,
    authorize: impl FnOnce(&str, &str) -> Result<Authorization>,
) -> Result<()> {
    let alias = prompt("Account alias")?;
    validate_alias(&alias)?;
    if repository
        .accounts()?
        .iter()
        .any(|account| account.alias == alias)
    {
        return Err(io::Error::other(format!("account alias already exists: {alias}")).into());
    }
    let (profile, secret) = select_profile(repository, store, None)?;
    let authorization = authorize(&profile.client_id, &secret)?;
    commit_account(
        repository,
        store,
        Uuid::new_v4(),
        &alias,
        &profile,
        authorization,
    )
}

fn commit_account(
    repository: &Repository,
    store: &dyn CredentialStore,
    id: Uuid,
    alias: &str,
    profile: &OAuthClientProfile,
    authorization: Authorization,
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
    store.set(
        CredentialId::AccountRefresh(account.id),
        &authorization.refresh_token,
    )?;
    println!("Reauthorized {alias} ({})", authorization.email);
    Ok(())
}

fn select_profile(
    repository: &Repository,
    store: &dyn CredentialStore,
    required_alias: Option<&str>,
) -> Result<(OAuthClientProfile, String)> {
    let alias = match required_alias {
        Some(alias) => alias.to_owned(),
        None => prompt("OAuth client profile alias")?,
    };
    let profile = repository
        .config()?
        .oauth_clients
        .into_iter()
        .find(|profile| profile.alias == alias);
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
                    let imported = import_client(&prompt("Desktop OAuth client JSON path")?)?;
                    if imported.client_id != profile.client_id {
                        return Err(io::Error::other(
                            "imported client ID does not match the saved profile",
                        )
                        .into());
                    }
                    store.set(id, &imported.client_secret)?;
                    imported.client_secret
                }
            };
            Ok((profile, secret))
        }
        None if required_alias.is_none() => {
            validate_alias(&alias)?;
            let imported = import_client(&prompt("Desktop OAuth client JSON path")?)?;
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

fn import_client(path: &str) -> Result<ImportedClient> {
    gmail::parse_desktop_client(&fs::read_to_string(Path::new(path))?)
}

fn prompt(label: &str) -> Result<String> {
    eprint!("{label}: ");
    io::stderr().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    let value = value.trim().to_owned();
    if value.is_empty() {
        Err(io::Error::other(format!("{label} is required")).into())
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, sync::Mutex};

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
            reauthorize_account(&repository, &store, "personal", |_, _| Ok(Authorization {
                email: "two@example.com".into(),
                refresh_token: "new".into()
            }))
            .is_err()
        );
        assert_eq!(
            store
                .get(CredentialId::AccountRefresh(account.id))
                .unwrap()
                .as_deref(),
            Some("old")
        );
        reauthorize_account(&repository, &store, "personal", |_, _| {
            Ok(Authorization {
                email: "ONE@EXAMPLE.COM".into(),
                refresh_token: "new".into(),
            })
        })
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
            reauthorize_account(&repository, &store, "personal", |_, _| {
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
            })
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

        let error = reauthorize_account(&repository, &store, "personal", |_, _| {
            panic!("OAuth must not start for a non-Gmail account")
        })
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
            )
            .is_err()
        );
        assert_eq!(store.get(CredentialId::AccountRefresh(id)).unwrap(), None);
        assert!(repository.accounts().unwrap().is_empty());
    }
}
