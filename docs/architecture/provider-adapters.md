# Provider adapters

## Interface

Provider adapters are compile-time Rust implementations of a core `MailProvider` abstraction.

Illustrative responsibilities:

```rust
trait MailProvider {
    // authenticate / identify account
    // discover unread inbox seeds / incremental history
    // fetch message/thread/attachment/raw content
    // inspect current message state
    // mark one message read
    // move one message to trash
}
```

The exact trait should be designed from domain requirements and tests, not copied literally from this sketch.

## Gmail v1

Gmail is the first built-in adapter and includes Google Workspace Gmail.

Direct REST/HTTP is preferred over a generated provider SDK. Use small `bit-mail`-owned DTOs and keep provider-specific types behind the adapter boundary.

Required Gmail API capabilities include conceptually:

- profile/account identity;
- messages list/get;
- threads get;
- attachment get;
- history list;
- message modify for read state;
- message Trash operation.

## OAuth

Use `gmail.modify` only. OAuth client profiles are reusable across accounts; each account has distinct authorization/refresh token.

Secrets live in the OS credential store, not the repository.

## Resilience

Adapters own retries, rate-limit/backoff behavior, request timeout policy, error mapping, and redacted logging. Bounded concurrency is required for pull/push.

## Tests

Core logic should be testable with a fake provider. Gmail adapter contract tests should use a mock HTTP server. Live Gmail tests are opt-in only.
