# Gmail setup

These steps were verified on 2026-08-31 against Google's current
[Gmail quickstart](https://developers.google.com/workspace/gmail/api/quickstart/python),
[desktop OAuth flow](https://developers.google.com/identity/protocols/oauth2/native-app),
and [scope reference](https://developers.google.com/workspace/gmail/api/auth/scopes).

## Google Cloud setup

1. Create or select a Google Cloud project and enable the Gmail API.
2. Open **Google Auth Platform** and configure Branding, Audience, and Data Access.
3. Choose **External** for personal Gmail or mixed Gmail/Workspace use. Add each
   account as a test user while the app remains in Testing. Choose **Internal**
   only when every mailbox belongs to the same Google Workspace organization.
4. Add only `https://www.googleapis.com/auth/gmail.modify` under Data Access.
5. Open **Clients**, create a **Desktop app**, and download its JSON file.

Desktop clients use a random loopback `127.0.0.1` redirect. Google's deprecated
manual copy/paste flow is not used.

External apps in Testing receive authorizations that expire after seven days
when restricted Gmail scopes are requested. Use an appropriate production
publishing configuration for durable use.

## OAuth model

`bit-mail` is a BYO-OAuth local CLI for technical users.

Required scope:

```text
https://www.googleapis.com/auth/gmail.modify
```

Do not request the broader `https://mail.google.com/` scope.

## OAuth client profiles

One OAuth client profile can authorize multiple Gmail accounts. Each Gmail mailbox receives its own refresh token/account authorization.

Use an External OAuth profile for personal Gmail or mixed personal + Workspace accounts. An Internal profile is suitable for Workspace-only use inside one organization.

External profiles intended for durable use should not remain in Google's Testing status because testing-mode authorization may have short refresh-token lifetime. `bit-mail` itself does not manage Google Cloud publishing configuration.

## Connect

Expected flow:

```bash
bit-mail connect
```

Each prompt explains the requested value and accepts a displayed default with
Enter. Account aliases default through `personal`, `work`, and `group`. An
existing OAuth client profile can be reused for multiple mailboxes; a new
profile prompts for the downloaded Desktop client JSON path.

The JSON path accepts `~/`. Keep that file outside the bit-mail repository;
`connect` rejects credential files inside it. Invalid aliases and credential
paths can be corrected without restarting the command. Each mailbox receives
its own keyring-backed refresh token. If the browser cannot be launched, open
the URL printed by `bit-mail` manually. Failed authorization creates no
account.

## Reauthorization

Expected recovery form:

```bash
bit-mail connect --reauthorize <alias>
```

`bit-mail doctor` should identify invalid/missing provider authorization and point to this path.

Reauthorization must select the same mailbox. On a new machine, it also asks
for the original Desktop client JSON when that profile's client secret is not
present in the local credential store.

`account remove --revoke-credentials` revokes the token at Google before
deleting it locally. Google revocation can invalidate other grants for the
same user and Cloud project; use `--keep-credentials` when that is not wanted.
