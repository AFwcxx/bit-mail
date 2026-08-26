# Architecture overview

## Goal

`bit-mail` is a deterministic local triage engine between an authoritative mail provider and a human/AI semantic decision layer.

```text
                 authoritative provider
                        Gmail
                          |
                    pull / push
                          |
                          v
                 +----------------+
                 |    bit-mail    |
                 | deterministic  |
                 +-------+--------+
                         |
                stable local corpus
                         |
            +------------+-------------+
            |                          |
          human                   AI harness
                               semantic only
```

## Responsibility split

### bit-mail owns

- repository/account discovery;
- OAuth/credential references;
- provider adapters and REST behavior;
- pull/backlog/history handling;
- MIME parsing and normalization;
- complete thread materialization;
- canonical message identity;
- stable harness-facing local content;
- work-item lifecycle;
- selections;
- Knowledge persistence;
- BLAKE3/Merkle integrity;
- structural index;
- repair/GC/cache rebuild;
- provider mutation during push;
- audit and diagnostics;
- machine-readable CLI capability discovery.

### AI harness owns

- summarization;
- semantic classification;
- deciding whether information matters;
- grouping related messages semantically;
- explaining recommendations;
- suggesting reusable Knowledge.

The harness does not own persistence mechanics and never manipulates provider state directly.

## Data categories

```text
Provider-derived cache
  -> disposable, authoritative copy can be re-fetched

Local intent
  -> pending/read/delete work items

Selections
  -> account-local working-set references

Knowledge
  -> durable user-approved preference data

Audit
  -> durable content-redacted operation history
```

## Core commands

```text
init      create repository
connect   authorize account
pull      provider -> local
stage     pending -> staged read/delete
unstage   staged -> pending
push      staged intent -> provider
repair    rebuild untrusted provider-derived context
gc        remove unreachable provider cache
```

No `sync` command exists. No mail send operation exists.
