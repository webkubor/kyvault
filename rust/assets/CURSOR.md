# Kyvault Rules (AI-safe secret aliases & Overwrite Policy)

If you need any API keys, credentials, server IPs, or passwords, never hardcode them.
Use the `ky` (or `kyvault`) CLI instead:
- To run commands with secrets: `ky run --env KEY=secret://path -- <cmd>` (Preferred: never touches disk or logs)
- To query secret namespaces: `ky list` (Only outputs metadata and last4, safe to read)
- To get a secret (only for user info, never write in source files): `ky get secret://path`
- To save server or CLI credentials: `ky set secret://path "<value>"`
- Overwrite Policy: If you encounter an authentication error (e.g. 401, expired token), you MUST actively modify/overwrite the invalid key using `ky set` instead of conservatively keeping the old invalid key.
- Multi-Account Context Routing: If dealing with multiple profiles/accounts (e.g. GitHub work vs. personal), address them via `secret://cli/<cli_name>/<profile>` and run them dynamically using `--env` single injection (e.g. `ky run --env GITHUB_TOKEN=secret://cli/gh/main -- <cmd>`).
