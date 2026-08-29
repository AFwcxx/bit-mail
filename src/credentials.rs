use uuid::Uuid;

use crate::{
    Result,
    repository::{AccountConfig, CredentialRevoker},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CredentialId {
    OAuthClient(Uuid),
    AccountRefresh(Uuid),
}

pub trait CredentialStore {
    fn get(&self, id: CredentialId) -> Result<Option<String>>;
    fn set(&self, id: CredentialId, secret: &str) -> Result<()>;
    fn delete(&self, id: CredentialId) -> Result<()>;
}

pub struct KeyringStore {
    repository_id: Uuid,
}

impl KeyringStore {
    pub fn new(repository_id: Uuid) -> Self {
        Self { repository_id }
    }

    fn entry(&self, id: CredentialId) -> Result<keyring::Entry> {
        let user = match id {
            CredentialId::OAuthClient(id) => format!("oauth-client:{id}"),
            CredentialId::AccountRefresh(id) => format!("gmail-refresh:{id}"),
        };
        keyring::Entry::new(&format!("bit-mail:{}", self.repository_id), &user)
            .map_err(keyring_error)
    }
}

impl CredentialStore for KeyringStore {
    fn get(&self, id: CredentialId) -> Result<Option<String>> {
        match self.entry(id)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keyring_error(error)),
        }
    }

    fn set(&self, id: CredentialId, secret: &str) -> Result<()> {
        self.entry(id)?.set_password(secret).map_err(keyring_error)
    }

    fn delete(&self, id: CredentialId) -> Result<()> {
        match self.entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keyring_error(error)),
        }
    }
}

fn keyring_error(_error: keyring::Error) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(
        "secure credential store unavailable; see docs/setup/linux.md or docs/setup/macos.md",
    )
    .into()
}

pub struct GoogleCredentialRevoker<'a> {
    pub store: &'a dyn CredentialStore,
}

impl GoogleCredentialRevoker<'_> {
    fn revoke_at(&self, account: &AccountConfig, endpoint: &str) -> Result<()> {
        let id = CredentialId::AccountRefresh(account.id);
        let token = self.store.get(id)?.ok_or_else(|| {
            std::io::Error::other(
                "Gmail refresh token is missing; reauthorize or use --keep-credentials",
            )
        })?;
        let response = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?
            .post(endpoint)
            .form(&[("token", token.as_str())])
            .send()?;
        if !response.status().is_success() {
            let error = response.text().unwrap_or_default();
            if !error.contains("invalid_token") {
                return Err(std::io::Error::other("Google credential revocation failed").into());
            }
        }
        self.store.delete(id)
    }
}

impl CredentialRevoker for GoogleCredentialRevoker<'_> {
    fn revoke(&self, _repository_id: Uuid, account: &AccountConfig) -> Result<()> {
        self.revoke_at(account, "https://oauth2.googleapis.com/revoke")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::Mutex,
        thread,
    };

    struct MemoryStore(Mutex<Option<String>>);

    impl CredentialStore for MemoryStore {
        fn get(&self, _id: CredentialId) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn set(&self, _id: CredentialId, secret: &str) -> Result<()> {
            *self.0.lock().unwrap() = Some(secret.to_owned());
            Ok(())
        }
        fn delete(&self, _id: CredentialId) -> Result<()> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    fn revocation_server(status: &str, body: &str) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/revoke", listener.local_addr().unwrap());
        let status = status.to_owned();
        let body = body.to_owned();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
            }
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        (endpoint, server)
    }

    fn account() -> AccountConfig {
        AccountConfig {
            schema_version: 1,
            id: Uuid::nil(),
            alias: "personal".into(),
            provider: "gmail".into(),
            provider_identity: Some("person@example.com".into()),
            credential_profile: Some("google".into()),
        }
    }

    #[test]
    fn credential_ids_are_scoped_by_kind() {
        let id = Uuid::nil();
        assert_ne!(
            CredentialId::OAuthClient(id),
            CredentialId::AccountRefresh(id)
        );
    }

    #[test]
    fn credential_diagnostics_never_include_backend_details() {
        let error = keyring_error(keyring::Error::Invalid(
            "credential".into(),
            "sentinel-secret".into(),
        ));
        assert!(!error.to_string().contains("sentinel-secret"));
        assert!(error.to_string().contains("docs/setup/linux.md"));
    }

    #[test]
    fn google_revocation_deletes_only_successful_or_already_invalid_tokens() {
        for (status, body, deleted) in [
            ("200 OK", "", true),
            ("400 Bad Request", r#"{"error":"invalid_token"}"#, true),
            ("400 Bad Request", r#"{"error":"invalid_request"}"#, false),
        ] {
            let store = MemoryStore(Mutex::new(Some("refresh-token".into())));
            let revoker = GoogleCredentialRevoker { store: &store };
            let (endpoint, server) = revocation_server(status, body);
            assert_eq!(revoker.revoke_at(&account(), &endpoint).is_ok(), deleted);
            assert_eq!(store.0.lock().unwrap().is_none(), deleted);
            server.join().unwrap();
        }
    }
}
