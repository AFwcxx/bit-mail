# Design decisions

This is a compact decision log. Detailed rationale lives in the linked architecture/design docs and `docs/requirements.md`.

| Decision | v1 choice |
|---|---|
| Project name | `bit-mail` |
| Language | Rust 2024 |
| Platforms | Linux, macOS |
| License | Apache-2.0 |
| Core LLM dependency | None |
| Provider v1 | Gmail / Workspace Gmail |
| Provider API | Direct REST by default |
| Provider extension | Compile-time adapter trait |
| Gmail scope | `gmail.modify` only |
| Credentials | OS credential store, no plaintext fallback |
| Runtime config/data | Repository-scoped, initialized with `bit-mail init` |
| Account identity | Immutable UUID + mutable alias |
| Multiple accounts/repo | Yes, isolated |
| Duplicate mailbox/repo | Prohibited |
| Provider source of truth | Always |
| Canonical unit | Individual provider message |
| Thread context | Complete, no truncation |
| Context-only messages | Stored canonically, no work item |
| Workflow state | `pending | read | delete` |
| Mutation vocabulary | `stage` / `unstage` |
| Provider directions | `pull` / `push`, no `sync` |
| Pull with staged state | Prohibited for affected account |
| Delete | Move one message to Trash |
| Read | Remove provider unread state |
| Whole-thread delete | Not supported |
| Attachments | Keep already-downloaded bytes; remote lazy fetch |
| Stable AI files | `content.md`, `metadata.json`, attachments |
| Search | Ordinary filesystem tools; no generic FTS v1 |
| Structural DB | Disposable SQLite |
| Selections | Persistent account-scoped ID sets |
| Knowledge | One Markdown file/item, repository-global or account-specific |
| AI annotations | No general subsystem |
| Integrity | BLAKE3 hierarchical Merkle tree |
| Repair | Re-fetch provider truth; thread-level for thread corruption |
| Audit | Append-only, metadata-only, indefinite |
| Telemetry | None |
| Harness push | Only after explicit user authorization |
| Runtime templates | Embedded, binary-version-matched |
| Template updates | Binary upgrade only in v1 |
| Distribution | GitHub binaries + Cargo/source |
