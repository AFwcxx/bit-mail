#![cfg(feature = "live-gmail-tests")]

use std::env;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use bit_mail::{
    gmail::GmailClient,
    provider::{MailProvider, PushMessageState},
};
use serde::Deserialize;

const CONFIRMATION: &str = "I_UNDERSTAND_THIS_USES_A_DEDICATED_MAILBOX";
const BASE: &str = "https://gmail.googleapis.com";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    email_address: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Inserted {
    id: String,
    thread_id: String,
}

struct TrashOnDrop {
    http: reqwest::blocking::Client,
    token: String,
    id: String,
}

impl Drop for TrashOnDrop {
    fn drop(&mut self) {
        let _ = self
            .http
            .post(format!(
                "{BASE}/gmail/v1/users/me/messages/{}/trash",
                self.id
            ))
            .bearer_auth(&self.token)
            .send();
    }
}

#[test]
#[ignore = "requires explicit opt-in and a dedicated Gmail test mailbox"]
fn controlled_message_lifecycle_never_lists_arbitrary_mail() {
    assert_eq!(
        env::var("BIT_MAIL_LIVE_GMAIL_CONFIRM").as_deref(),
        Ok(CONFIRMATION)
    );
    let expected = env::var("BIT_MAIL_LIVE_GMAIL_ACCOUNT")
        .expect("BIT_MAIL_LIVE_GMAIL_ACCOUNT must name the dedicated mailbox");
    let token = env::var("BIT_MAIL_LIVE_GMAIL_ACCESS_TOKEN")
        .expect("BIT_MAIL_LIVE_GMAIL_ACCESS_TOKEN is required");
    let http = reqwest::blocking::Client::new();
    let profile: Profile = http
        .get(format!("{BASE}/gmail/v1/users/me/profile"))
        .bearer_auth(&token)
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(
        profile.email_address, expected,
        "dedicated-mailbox guard failed"
    );

    let nonce = uuid::Uuid::new_v4();
    let raw = format!(
        "From: bit-mail-live-test@invalid.example\r\nTo: {expected}\r\nSubject: bit-mail controlled live test {nonce}\r\nMessage-ID: <{nonce}@bit-mail.invalid>\r\nDate: Thu, 01 Jan 1970 00:00:00 +0000\r\nContent-Type: text/plain; charset=utf-8\r\n\r\ncontrolled bit-mail test fixture\r\n"
    );
    let inserted: Inserted = http
        .post(format!("{BASE}/gmail/v1/users/me/messages"))
        .bearer_auth(&token)
        .json(&serde_json::json!({
            "raw": URL_SAFE_NO_PAD.encode(raw),
            "labelIds": ["INBOX", "UNREAD"]
        }))
        .send()
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .unwrap();
    let cleanup = TrashOnDrop {
        http,
        token: token.clone(),
        id: inserted.id.clone(),
    };

    let gmail = GmailClient::new(token, BASE).unwrap();
    let reference = gmail.message_ref(&inserted.id).unwrap();
    assert_eq!(reference.thread_id, inserted.thread_id);
    let thread = gmail.thread(&inserted.thread_id).unwrap();
    assert!(
        thread
            .messages
            .iter()
            .any(|message| message.provider_message_id == inserted.id)
    );
    assert_eq!(
        gmail.push_state(&inserted.id).unwrap(),
        Some(PushMessageState {
            unread: true,
            trash: false
        })
    );
    assert!(!gmail.mark_read(&inserted.id).unwrap().unread);
    assert!(gmail.trash(&inserted.id).unwrap().trash);
    drop(cleanup);
}
