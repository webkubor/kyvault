# Show HN — draft only, not submitted

This is the single launch draft retained after scope reduction. The other platform variants are deferred, not researched-ready copy.

## Title

Show HN: Kyvault – Local secret storage with aliases for CLI environment injection

Alternatives:

- Show HN: Kyvault – Pass secret references to commands instead of plaintext keys
- Show HN: Kyvault – A Rust CLI secret manager for AI-assisted terminal work
- Show HN: Kyvault – Keep API keys in a local encrypted store, inject them at runtime

URL: https://github.com/webkubor/kyvault

## Introductory comment

When an AI coding assistant needs to run a command with an API key, copying the value into the conversation or asking it to read a plaintext .env file exposes that value to the assistant's context. I built kyvault to let the assistant work with a reference instead:

```sh
ky run --env TOKEN=secret://example/api-key -- your-command
```

Kyvault resolves the reference and passes the decrypted value in the child process environment. The command the assistant constructs only contains the reference. It is a Rust CLI with AES-256-GCM encrypted local storage, a local browser UI (`ky ui`), and a terminal wizard (`ky wizard`).

The boundary matters: this is environment injection, not a sandbox. The child can read and print its environment, and `ky get` deliberately returns plaintext. An assistant allowed to run arbitrary commands can still leak a value. The useful change is that ordinary authenticated commands no longer require handing the value to the model first.

The default file backend decrypts locally. API validation, updates, optional GitLab synchronization, and the optional D1 backend involve network access. This is a local developer tool, not a replacement for a centrally administered team KMS.

Source and installation: https://github.com/webkubor/kyvault

I would appreciate feedback on the command workflow and on which exposure paths remain easy to overlook.

## HN convention evidence and owner action

Read [Show HN guidelines](https://news.ycombinator.com/showhn.html), checked 2026-10-20. They say “things people can run on their computers” are on topic; “The project must be something you've worked on personally and which you're around to discuss”; and titles begin with “Show HN”. The owner should submit only when available to answer implementation questions. No solicitation of votes or coordinated comments.

This draft uses the runnable repository as the URL, rather than a signup page. Review first-person wording as the actual maker before posting.

## Claim evidence

- Rust CLI, MIT: [manifest](../../rust/Cargo.toml).
- AES-256-GCM file encryption: [crypto](../../rust/src/crypto.rs).
- `ky` short command: [installer](../../install.sh), symlink creation at lines 94 and 136 during inspection.
- Reference resolution and subprocess environment; plaintext `get`: [CLI](../../rust/src/main.rs), `Cmd::Run` and `Cmd::Get`.
- Local default versus optional D1: [CLI](../../rust/src/main.rs), `Backend::select`.
- Local browser binding: [UI](../../rust/src/ui.rs), `TcpListener::bind` to `127.0.0.1`.
- Terminal wizard: [wizard](../../rust/src/wizard.rs) and [SKILL](../../SKILL.md).
- Network operations: [provider validation](../../rust/src/providers.rs), [updates](../../rust/src/update.rs), [D1](../../rust/src/d1.rs), [GitLab integration](../../rust/src/gitlab.rs).

No statement about any provider's training-data policy is made: the repository cannot establish that policy.
