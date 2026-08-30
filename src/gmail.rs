use std::{
    collections::{BTreeMap, HashSet},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::Command,
    sync::atomic::{AtomicU32, Ordering},
    time::{Duration, Instant, SystemTime},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use mail_parser::MessageParser;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl,
    RefreshToken, Scope, TokenResponse, TokenUrl, basic::BasicClient,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use url::Url;

use crate::{
    Result,
    credentials::{CredentialId, CredentialStore},
    provider::{
        HistoryPage, MailProvider, MessageRef, MessageState, Page, ProviderError, ProviderErrorKind,
    },
    repository::{AccountConfig, Repository},
    storage::{
        Address, MailboxFlags, MessageInput, MimePartInput, RemoteAttachment, ThreadInput,
        TransferEncoding,
    },
};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";

#[derive(Deserialize)]
struct DesktopClientFile {
    installed: DesktopClient,
}

#[derive(Deserialize)]
struct DesktopClient {
    client_id: String,
    client_secret: String,
    auth_uri: String,
    token_uri: String,
}

pub struct ImportedClient {
    pub client_id: String,
    pub client_secret: String,
}

pub struct Authorization {
    pub email: String,
    pub refresh_token: String,
}

pub fn parse_desktop_client(json: &str) -> Result<ImportedClient> {
    let file: DesktopClientFile = serde_json::from_str(json)?;
    let client = file.installed;
    if client.client_id.trim().is_empty() || client.client_secret.trim().is_empty() {
        return Err(
            std::io::Error::other("Desktop OAuth client ID and secret are required").into(),
        );
    }
    if !matches!(
        client.auth_uri.as_str(),
        AUTH_URL | "https://accounts.google.com/o/oauth2/auth"
    ) || client.token_uri != TOKEN_URL
    {
        return Err(std::io::Error::other("unrecognized Google OAuth endpoints").into());
    }
    Ok(ImportedClient {
        client_id: client.client_id,
        client_secret: client.client_secret,
    })
}

pub fn authorize(client_id: &str, client_secret: &str) -> Result<Authorization> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let redirect = format!("http://127.0.0.1:{}/", listener.local_addr()?.port());
    let client = BasicClient::new(ClientId::new(client_id.to_owned()))
        .set_client_secret(ClientSecret::new(client_secret.to_owned()))
        .set_auth_uri(AuthUrl::new(AUTH_URL.to_owned())?)
        .set_token_uri(TokenUrl::new(TOKEN_URL.to_owned())?)
        .set_redirect_uri(RedirectUrl::new(redirect)?);
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (auth_url, state) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(SCOPE.to_owned()))
        .add_extra_param("access_type", "offline")
        .add_extra_param("prompt", "consent")
        .set_pkce_challenge(challenge)
        .url();
    if !open_browser(auth_url.as_str()) {
        eprintln!("Open this authorization URL:\n{auth_url}");
    }
    let code = wait_for_code(&listener, state.secret())?;
    let http = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?;
    let token = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(verifier)
        .request(&http)?;
    let refresh_token = token
        .refresh_token()
        .ok_or_else(|| std::io::Error::other("Google did not return a refresh token"))?
        .secret()
        .to_owned();
    let profile: GmailProfile = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?
        .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
        .bearer_auth(token.access_token().secret())
        .send()?
        .error_for_status()?
        .json()?;
    Ok(Authorization {
        email: profile.email_address.to_ascii_lowercase(),
        refresh_token,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GmailProfile {
    email_address: String,
    #[serde(default)]
    history_id: String,
}

pub struct GmailClient {
    http: reqwest::blocking::Client,
    base_url: String,
    access_token: String,
    retries: AtomicU32,
}

#[derive(Clone, Copy)]
enum Operation {
    Profile,
    ListMessages,
    ListHistory,
    MessageState,
    Thread,
    Attachment,
    RawMessage,
}

impl Operation {
    fn label(self) -> &'static str {
        match self {
            Self::Profile => "profile",
            Self::ListMessages => "list messages",
            Self::ListHistory => "list history",
            Self::MessageState => "message state",
            Self::Thread => "thread",
            Self::Attachment => "attachment",
            Self::RawMessage => "raw message",
        }
    }
}

impl GmailClient {
    pub fn new(access_token: impl Into<String>, base_url: impl Into<String>) -> Result<Self> {
        Ok(Self {
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            access_token: access_token.into(),
            retries: AtomicU32::new(0),
        })
    }

    fn get<T: DeserializeOwned>(
        &self,
        operation: Operation,
        path: &str,
        query: &[(&str, String)],
        missing: ProviderErrorKind,
    ) -> Result<T> {
        let mut url = Url::parse(&format!("{}/gmail/v1/users/me/{path}", self.base_url))?;
        url.query_pairs_mut()
            .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        let operation = operation.label();
        for attempt in 0..=3 {
            tracing::debug!(
                provider = "gmail",
                operation,
                attempt = attempt + 1,
                "provider request"
            );
            let sent = self
                .http
                .get(url.clone())
                .bearer_auth(&self.access_token)
                .send();
            match sent {
                Ok(response) if response.status().is_success() => {
                    return response.json().map_err(|_| {
                        ProviderError(
                            ProviderErrorKind::Permanent,
                            "Gmail returned malformed JSON",
                        )
                        .into()
                    });
                }
                Ok(response)
                    if response.status().as_u16() == 401 || response.status().as_u16() == 403 =>
                {
                    tracing::warn!(
                        provider = "gmail",
                        operation,
                        status = response.status().as_u16(),
                        "provider request failed"
                    );
                    return Err(ProviderError(
                        ProviderErrorKind::Authentication,
                        "Gmail authorization failed; reauthorize the account",
                    )
                    .into());
                }
                Ok(response) if response.status().as_u16() == 404 => {
                    tracing::warn!(
                        provider = "gmail",
                        operation,
                        status = 404,
                        "provider request failed"
                    );
                    return Err(ProviderError(missing, "Gmail object is unavailable").into());
                }
                Ok(response)
                    if response.status().as_u16() == 429 || response.status().is_server_error() =>
                {
                    if attempt == 3 {
                        tracing::warn!(
                            provider = "gmail",
                            operation,
                            status = response.status().as_u16(),
                            "provider request retry limit exceeded"
                        );
                        break;
                    }
                    let delay = response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(retry_after)
                        .unwrap_or(Duration::from_millis(250 << attempt));
                    self.retries.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(
                        provider = "gmail",
                        operation,
                        status = response.status().as_u16(),
                        attempt = attempt + 1,
                        "provider request retry"
                    );
                    std::thread::sleep(delay.min(Duration::from_secs(30)));
                }
                Ok(response) => {
                    tracing::warn!(
                        provider = "gmail",
                        operation,
                        status = response.status().as_u16(),
                        "provider request failed"
                    );
                    return Err(ProviderError(
                        ProviderErrorKind::Permanent,
                        "Gmail request failed",
                    )
                    .into());
                }
                Err(_) if attempt < 3 => {
                    self.retries.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!(
                        provider = "gmail",
                        operation,
                        attempt = attempt + 1,
                        "provider transport retry"
                    );
                    std::thread::sleep(Duration::from_millis(250 << attempt));
                }
                Err(_) => {
                    tracing::warn!(provider = "gmail", operation, "provider transport failed");
                    return Err(ProviderError(
                        ProviderErrorKind::Permanent,
                        "Gmail transport failed",
                    )
                    .into());
                }
            }
        }
        Err(ProviderError(ProviderErrorKind::Permanent, "Gmail retry limit exceeded").into())
    }
}

pub fn authorized_client(
    repository: &Repository,
    account: &AccountConfig,
    store: &dyn CredentialStore,
) -> Result<GmailClient> {
    if account.provider != "gmail" {
        return Err(std::io::Error::other(format!(
            "account {} is not a Gmail account",
            account.alias
        ))
        .into());
    }
    let profile_alias = account
        .credential_profile
        .as_deref()
        .ok_or_else(|| std::io::Error::other("account has no OAuth client profile"))?;
    let profile = repository
        .config()?
        .oauth_clients
        .into_iter()
        .find(|v| v.alias == profile_alias)
        .ok_or_else(|| std::io::Error::other("account OAuth client profile is missing"))?;
    let client_secret = store
        .get(CredentialId::OAuthClient(profile.id))?
        .ok_or_else(|| {
            std::io::Error::other("OAuth client secret is missing; reauthorize the account")
        })?;
    let refresh = store
        .get(CredentialId::AccountRefresh(account.id))?
        .ok_or_else(|| {
            std::io::Error::other("Gmail refresh token is missing; reauthorize the account")
        })?;
    let http = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let client = BasicClient::new(ClientId::new(profile.client_id))
        .set_client_secret(ClientSecret::new(client_secret))
        .set_token_uri(TokenUrl::new(TOKEN_URL.to_owned())?);
    let token = client
        .exchange_refresh_token(&RefreshToken::new(refresh))
        .request(&http)
        .map_err(|_| {
            std::io::Error::other("Gmail authorization refresh failed; reauthorize the account")
        })?;
    GmailClient::new(
        token.access_token().secret(),
        "https://gmail.googleapis.com",
    )
}

fn retry_after(value: &str) -> Option<Duration> {
    value
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
        .or_else(|| {
            let then = httpdate::parse_http_date(value).ok()?;
            Some(then.duration_since(SystemTime::now()).unwrap_or_default())
        })
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiMessage {
    id: String,
    thread_id: String,
    #[serde(default)]
    label_ids: Vec<String>,
    #[serde(default)]
    internal_date: String,
    payload: Option<ApiPart>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiPart {
    #[serde(default)]
    part_id: String,
    #[serde(default)]
    mime_type: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    headers: Vec<ApiHeader>,
    #[serde(default)]
    body: ApiBody,
    #[serde(default)]
    parts: Vec<ApiPart>,
}
#[derive(Clone, Deserialize, Serialize)]
struct ApiHeader {
    name: String,
    value: String,
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApiBody {
    #[serde(default)]
    attachment_id: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    data: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListResponse {
    #[serde(default)]
    messages: Vec<ApiMessage>,
    next_page_token: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadResponse {
    id: String,
    #[serde(default)]
    messages: Vec<ApiMessage>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryResponse {
    #[serde(default)]
    history: Vec<HistoryRecord>,
    next_page_token: Option<String>,
    history_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryRecord {
    #[serde(default)]
    messages: Vec<ApiMessage>,
    #[serde(default)]
    messages_added: Vec<HistoryMessage>,
    #[serde(default)]
    messages_deleted: Vec<HistoryMessage>,
    #[serde(default)]
    labels_added: Vec<HistoryMessage>,
    #[serde(default)]
    labels_removed: Vec<HistoryMessage>,
}
#[derive(Deserialize)]
struct HistoryMessage {
    message: ApiMessage,
}

impl MailProvider for GmailClient {
    fn retries(&self) -> u32 {
        self.retries.load(Ordering::Relaxed)
    }

    fn current_history_id(&self) -> Result<String> {
        let profile: GmailProfile = self.get(
            Operation::Profile,
            "profile",
            &[],
            ProviderErrorKind::Permanent,
        )?;
        Ok(profile.history_id)
    }

    fn unread_page(&self, page: Option<&str>, limit: u32) -> Result<Page<MessageRef>> {
        let mut query = vec![
            ("labelIds", "INBOX".into()),
            ("labelIds", "UNREAD".into()),
            ("maxResults", limit.min(500).to_string()),
        ];
        if let Some(page) = page {
            query.push(("pageToken", page.into()));
        }
        let value: ListResponse = self.get(
            Operation::ListMessages,
            "messages",
            &query,
            ProviderErrorKind::Permanent,
        )?;
        Ok(Page {
            items: value.messages.into_iter().map(message_ref).collect(),
            next_page: value.next_page_token,
        })
    }

    fn history_page(&self, start: &str, page: Option<&str>) -> Result<HistoryPage> {
        let mut query = vec![
            ("startHistoryId", start.into()),
            ("maxResults", "500".into()),
        ];
        if let Some(page) = page {
            query.push(("pageToken", page.into()));
        }
        let value: HistoryResponse = self.get(
            Operation::ListHistory,
            "history",
            &query,
            ProviderErrorKind::HistoryExpired,
        )?;
        let mut seen = HashSet::new();
        let mut changed = Vec::new();
        for record in value.history {
            for message in record
                .messages
                .into_iter()
                .chain(record.messages_added.into_iter().map(|v| v.message))
                .chain(record.messages_deleted.into_iter().map(|v| v.message))
                .chain(record.labels_added.into_iter().map(|v| v.message))
                .chain(record.labels_removed.into_iter().map(|v| v.message))
            {
                if seen.insert(message.id.clone()) {
                    changed.push(message_ref(message));
                }
            }
        }
        Ok(HistoryPage {
            changed,
            next_page: value.next_page_token,
            history_id: value.history_id,
        })
    }

    fn message_state(&self, id: &str) -> Result<MessageState> {
        let result = self.get::<ApiMessage>(
            Operation::MessageState,
            &format!("messages/{id}"),
            &[("format", "minimal".into())],
            ProviderErrorKind::Missing,
        );
        match result {
            Ok(message) => Ok(if actionable(&message.label_ids) {
                MessageState::Actionable
            } else {
                MessageState::Inactive
            }),
            Err(error)
                if error
                    .downcast_ref::<ProviderError>()
                    .is_some_and(|v| v.0 == ProviderErrorKind::Missing) =>
            {
                Ok(MessageState::Missing)
            }
            Err(error) => Err(error),
        }
    }

    fn message_ref(&self, id: &str) -> Result<MessageRef> {
        let message: ApiMessage = self.get(
            Operation::MessageState,
            &format!("messages/{id}"),
            &[("format", "minimal".into())],
            ProviderErrorKind::Missing,
        )?;
        Ok(message_ref(message))
    }

    fn thread(&self, id: &str) -> Result<ThreadInput> {
        let thread: ThreadResponse = self.get(
            Operation::Thread,
            &format!("threads/{id}"),
            &[("format", "full".into())],
            ProviderErrorKind::Missing,
        )?;
        let messages = thread
            .messages
            .into_iter()
            .map(api_message)
            .collect::<Result<_>>()?;
        Ok(ThreadInput {
            provider: "gmail".into(),
            provider_thread_id: thread.id,
            messages,
        })
    }

    fn attachment(&self, message_id: &str, attachment_id: &str) -> Result<Vec<u8>> {
        let body: ApiBody = self.get(
            Operation::Attachment,
            &format!("messages/{message_id}/attachments/{attachment_id}"),
            &[],
            ProviderErrorKind::Missing,
        )?;
        decode(&body.data)
    }

    fn raw(&self, message_id: &str) -> Result<Vec<u8>> {
        #[derive(Deserialize)]
        struct Raw {
            raw: String,
        }
        let raw: Raw = self.get(
            Operation::RawMessage,
            &format!("messages/{message_id}"),
            &[("format", "raw".into())],
            ProviderErrorKind::Missing,
        )?;
        decode(&raw.raw)
    }
}

fn message_ref(message: ApiMessage) -> MessageRef {
    MessageRef {
        id: message.id,
        thread_id: message.thread_id,
    }
}
fn actionable(labels: &[String]) -> bool {
    labels.iter().any(|v| v == "INBOX") && labels.iter().any(|v| v == "UNREAD")
}
fn decode(value: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(value.trim_end_matches('='))
        .map_err(|_| {
            ProviderError(
                ProviderErrorKind::Permanent,
                "Gmail returned invalid base64url data",
            )
            .into()
        })
}

fn api_message(message: ApiMessage) -> Result<MessageInput> {
    let headers = message
        .payload
        .as_ref()
        .map(|p| &p.headers[..])
        .unwrap_or(&[]);
    let parsed_headers = headers
        .iter()
        .map(|h| format!("{}: {}\r\n", h.name, h.value))
        .collect::<String>()
        + "\r\n";
    let parsed = MessageParser::default().parse_headers(parsed_headers.as_bytes());
    let addresses = |value: Option<&mail_parser::Address<'_>>| {
        value
            .into_iter()
            .flat_map(|v| v.iter())
            .filter_map(|v| {
                Some(Address {
                    name: v.name.as_ref().map(|v| v.to_string()),
                    address: v.address.as_ref()?.to_string(),
                })
            })
            .collect()
    };
    let source = serde_json::to_value(&message)?;
    let flags = MailboxFlags {
        inbox: message.label_ids.iter().any(|v| v == "INBOX"),
        unread: message.label_ids.iter().any(|v| v == "UNREAD"),
        sent: message.label_ids.iter().any(|v| v == "SENT"),
        trash: message.label_ids.iter().any(|v| v == "TRASH"),
    };
    let received_at_ms = message.internal_date.parse()?;
    Ok(MessageInput {
        provider_message_id: message.id,
        provider_thread_id: message.thread_id,
        received_at_ms,
        sent_at_ms: parsed
            .as_ref()
            .and_then(|v| v.date())
            .map(|v| v.to_timestamp() * 1000),
        subject: parsed.as_ref().and_then(|v| v.subject()).map(str::to_owned),
        from: addresses(parsed.as_ref().and_then(|v| v.from())),
        to: addresses(parsed.as_ref().and_then(|v| v.to())),
        cc: addresses(parsed.as_ref().and_then(|v| v.cc())),
        bcc: addresses(parsed.as_ref().and_then(|v| v.bcc())),
        reply_to: addresses(parsed.as_ref().and_then(|v| v.reply_to())),
        rfc_message_id: parsed
            .as_ref()
            .and_then(|v| v.message_id())
            .map(str::to_owned),
        flags,
        parts: message
            .payload
            .into_iter()
            .map(|v| api_part(v, "root"))
            .collect::<Result<_>>()?,
        provider_source: source,
    })
}

fn api_part(part: ApiPart, fallback: &str) -> Result<MimePartInput> {
    let id = if part.part_id.is_empty() {
        fallback.to_owned()
    } else {
        part.part_id
    };
    let headers = part
        .headers
        .into_iter()
        .map(|v| (v.name, v.value))
        .collect::<BTreeMap<_, _>>();
    let body = if part.body.data.is_empty() {
        None
    } else {
        Some(decode(&part.body.data)?)
    };
    let remote = (!part.body.attachment_id.is_empty()).then_some(RemoteAttachment {
        provider_attachment_id: part.body.attachment_id,
        size: part.body.size,
    });
    let parts = part
        .parts
        .into_iter()
        .enumerate()
        .map(|(index, child)| api_part(child, &format!("{id}.{index}")))
        .collect::<Result<_>>()?;
    Ok(MimePartInput {
        id,
        mime_type: part.mime_type,
        headers,
        filename: (!part.filename.is_empty()).then_some(part.filename),
        transfer_encoding: TransferEncoding::None,
        body,
        remote,
        parts,
    })
}

fn open_browser(url: &str) -> bool {
    let command = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Command::new(command)
        .arg(url)
        .status()
        .is_ok_and(|status| status.success())
}

fn wait_for_code(listener: &TcpListener, expected_state: &str) -> Result<String> {
    wait_for_code_until(
        listener,
        expected_state,
        Instant::now() + Duration::from_secs(300),
    )
}

enum Callback {
    Code(String),
    Denied(String),
    Ignore,
}

fn wait_for_code_until(
    listener: &TcpListener,
    expected_state: &str,
    deadline: Instant,
) -> Result<String> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                stream.set_read_timeout(Some(remaining))?;
                stream.set_write_timeout(Some(remaining))?;
                match parse_callback(stream, expected_state) {
                    Ok(Callback::Code(code)) => return Ok(code),
                    Ok(Callback::Denied(error)) => {
                        return Err(std::io::Error::other(format!(
                            "Google authorization failed: {error}"
                        ))
                        .into());
                    }
                    Ok(Callback::Ignore) | Err(_) => {}
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(error.into()),
        }
    }
    Err(std::io::Error::other("timed out waiting for OAuth callback").into())
}

fn parse_callback(mut stream: TcpStream, expected_state: &str) -> Result<Callback> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?)
        .take(8192)
        .read_line(&mut line)?;
    let mut request = line.split_whitespace();
    let mut result = || {
        if request.next() != Some("GET") {
            return Callback::Ignore;
        }
        let Some(target) = request.next() else {
            return Callback::Ignore;
        };
        let Ok(url) = Url::parse(&format!("http://localhost{target}")) else {
            return Callback::Ignore;
        };
        if url.path() != "/" {
            return Callback::Ignore;
        }
        let value = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        if value("state").as_deref() != Some(expected_state) {
            Callback::Ignore
        } else if let Some(error) = value("error") {
            Callback::Denied(error)
        } else {
            value("code").map_or(Callback::Ignore, Callback::Code)
        }
    };
    let result = result();
    let message = if matches!(result, Callback::Code(_)) {
        "Authorization complete. Return to bit-mail."
    } else {
        "Authorization failed. Return to bit-mail."
    };
    let _ = write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}",
        message.len()
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::SocketAddr, thread};

    #[test]
    fn request_log_operations_cannot_include_provider_identifiers() {
        for operation in [
            Operation::Profile,
            Operation::ListMessages,
            Operation::ListHistory,
            Operation::MessageState,
            Operation::Thread,
            Operation::Attachment,
            Operation::RawMessage,
        ] {
            assert!(
                operation
                    .label()
                    .chars()
                    .all(|character| character.is_ascii_lowercase() || character == ' '),
                "request diagnostics must use fixed content-free labels"
            );
        }
    }

    fn callback_listener() -> TcpListener {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        listener
    }

    fn send_callback(address: SocketAddr, target: &str) {
        let mut stream = TcpStream::connect(address).unwrap();
        write!(stream, "GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    }

    #[test]
    fn desktop_json_must_use_google_endpoints() {
        let valid = r#"{"installed":{"client_id":"id","client_secret":"secret","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token"}}"#;
        let client = parse_desktop_client(valid).expect("valid Desktop JSON");
        assert_eq!(client.client_id, "id");
        assert!(
            parse_desktop_client(&valid.replace("oauth2.googleapis.com", "example.com")).is_err()
        );
    }

    #[test]
    fn callback_ignores_wrong_state_before_accepting_the_valid_code() {
        let listener = callback_listener();
        let address = listener.local_addr().unwrap();
        let sender = thread::spawn(move || {
            send_callback(address, "/?code=wrong&state=wrong");
            send_callback(address, "/?code=right&state=expected");
        });
        assert_eq!(
            wait_for_code_until(
                &listener,
                "expected",
                Instant::now() + Duration::from_secs(1)
            )
            .unwrap(),
            "right"
        );
        sender.join().unwrap();
    }

    #[test]
    fn callback_denial_is_terminal() {
        let listener = callback_listener();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || send_callback(address, "/?error=access_denied&state=expected"));
        let error = wait_for_code_until(
            &listener,
            "expected",
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap_err();
        assert!(error.to_string().contains("access_denied"));
    }

    #[test]
    fn idle_callback_connection_cannot_outlive_the_deadline() {
        let listener = callback_listener();
        let _stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let error = wait_for_code_until(
            &listener,
            "expected",
            Instant::now() + Duration::from_millis(100),
        )
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
    }

    #[test]
    fn rest_client_retries_rate_limits_and_preserves_seed_filters() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, body) in [
                ("429 Too Many Requests", ""),
                ("200 OK", r#"{"messages":[{"id":"m","threadId":"t"}]}"#),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                requests.push(request);
                write!(stream, "HTTP/1.1 {status}\r\nRetry-After: 0\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            requests
        });
        let client = GmailClient::new("token", base).unwrap();
        let page = client.unread_page(None, 600).unwrap();
        assert_eq!(page.items[0].id, "m");
        assert_eq!(client.retries(), 1);
        let requests = server.join().unwrap();
        assert!(requests[0].contains("labelIds=INBOX&labelIds=UNREAD&maxResults=500"));
    }

    #[test]
    fn gmail_full_message_maps_embedded_and_remote_parts() {
        let message: ApiMessage = serde_json::from_str(r#"{"id":"m","threadId":"t","labelIds":["INBOX","UNREAD"],"internalDate":"1000","payload":{"mimeType":"multipart/mixed","headers":[{"name":"Subject","value":"Hello"}],"parts":[{"partId":"0","mimeType":"text/plain","body":{"size":4,"data":"Ym9keQ"}},{"partId":"1","mimeType":"application/pdf","filename":"a.pdf","body":{"attachmentId":"remote","size":3}}]}}"#).unwrap();
        let mapped = api_message(message).unwrap();
        assert_eq!(mapped.subject.as_deref(), Some("Hello"));
        assert_eq!(
            mapped.parts[0].parts[0].body.as_deref(),
            Some(b"body".as_slice())
        );
        assert_eq!(
            mapped.parts[0].parts[1]
                .remote
                .as_ref()
                .unwrap()
                .provider_attachment_id,
            "remote"
        );
    }

    #[test]
    fn rest_client_pages_history_and_fetches_complete_thread() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let bodies = [
                r#"{"history":[{"labelsAdded":[{"message":{"id":"m","threadId":"t"}}]}],"nextPageToken":"next","historyId":"2"}"#,
                r#"{"history":[],"historyId":"3"}"#,
                r#"{"id":"t","messages":[{"id":"m","threadId":"t","labelIds":["INBOX","UNREAD"],"internalDate":"1000","payload":{"partId":"0","mimeType":"text/plain","body":{"size":4,"data":"Ym9keQ"}}}]}"#,
            ];
            let mut requests = Vec::new();
            for body in bodies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut request)
                    .unwrap();
                requests.push(request);
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
            requests
        });
        let client = GmailClient::new("token", base).unwrap();
        let first = client.history_page("1", None).unwrap();
        assert_eq!(first.changed[0].id, "m");
        let second = client
            .history_page("1", first.next_page.as_deref())
            .unwrap();
        assert_eq!(second.history_id, "3");
        let thread = client.thread("t").unwrap();
        assert_eq!(
            thread.messages[0].parts[0].body.as_deref(),
            Some(b"body".as_slice())
        );
        let requests = server.join().unwrap();
        assert!(requests[1].contains("pageToken=next"));
        assert!(requests[2].contains("threads/t?format=full"));
    }
}
