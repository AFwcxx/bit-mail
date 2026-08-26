# Gmail setup

> This document records the intended v1 setup model. Exact Google Console screenshots/labels should be verified against current Google documentation during implementation/release documentation work.

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

1. choose Gmail;
2. choose account alias;
3. select an existing OAuth client profile or import a new Desktop OAuth client JSON;
4. store OAuth client secret material in the OS credential store;
5. run interactive browser authorization;
6. fetch authenticated Gmail profile;
7. reject duplicate mailbox in this repository;
8. store refresh token securely;
9. initialize account state.

## Reauthorization

Expected recovery form:

```bash
bit-mail connect --reauthorize <alias>
```

`bit-mail doctor` should identify invalid/missing provider authorization and point to this path.
