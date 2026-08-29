use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::Command,
    time::{Duration, Instant},
};

use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl,
    Scope, TokenResponse, TokenUrl, basic::BasicClient,
};
use serde::Deserialize;
use url::Url;

use crate::Result;

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
}
