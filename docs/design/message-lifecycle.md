# Message lifecycle

## Pull

```text
Gmail INBOX+UNREAD seed
        |
        v
fetch complete thread
        |
        v
materialize each provider message once
        |
        +--> context-only canonical message (no work item)
        |
        +--> unread Inbox canonical message -> pending work item
```

## Decision

```text
pending
  |\
  | +-- stage delete --> staged delete
  +---- stage read ----> staged read

staged read/delete --unstage--> pending
```

Staging is local only.

## Push

```text
staged read
  -> provider preflight
  -> remove UNREAD if necessary
  -> success/no-op

staged delete
  -> provider preflight
  -> move one message to Trash if necessary
  -> success/no-op
```

Provider object missing is resolved locally with audit/warning rather than recreated.

## Cleanup

Successful work item removal triggers reachability analysis. Complete thread context remains while any actionable work item in the thread exists. When none remain, provider-derived thread cache becomes garbage-collectable.

## Pull blocking

Pending does not block pull. Any staged read/delete for the selected account blocks pull until pushed or unstaged.
