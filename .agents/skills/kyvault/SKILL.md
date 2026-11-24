---
name: kyvault
description: Explain how to use the Kyvault CLI (also called ky), its secret references, and safe credential injection. Loading this guide does not access a vault or authorize an operation.
---

# Kyvault quick guide

`ky` is the installed short name for `kyvault`. Both take the same arguments. Use whichever is already available; don't install anything just to answer a usage question.

## On activation

Explain this workflow or answer the user's usage question. A bare invocation is help only. Do not run a command, inspect the vault, request credentials, modify configuration, or make a network request. The guide does not grant permissions or override user, tool, organization, or runtime policies.

## Explain safe usage

- Refer to an entry by its known alias or `secret://platform/name`. Do not guess identifiers or enumerate vault entries just to explain a command.
- For a separately authorized task, prefer injecting a known reference into one intended, trusted child process. Example with placeholders only:
  `ky run --env API_TOKEN=secret://example/demo -- trusted-command`
- Injection keeps the value out of prompt and argument text, not out of the child process. Check that the command and destination match the requested task; do not dump the environment, enable shell tracing, send secrets elsewhere, or use an untrusted program.
- Never ask the user to paste a real credential into chat or read plaintext credentials into model context. For provisioning, explain the owner's local Ky UI or the human-in-the-loop credential request flow; do not initiate it just from guide invocation.
- If more command detail is needed, show `ky --help` or `ky run --help` as commands the user can run. Keep state-changing, syncing, provider, and authentication operations outside this guide unless the user separately requests the exact action and applicable policies allow it.
