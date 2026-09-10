
# Kyvault Rules (AI-safe secret aliases & Overwrite Policy)
If you need any API keys, credentials, server IPs or passwords, do not hardcode them.
Use the `cs secrets` tool (kyvault) instead:
- To run commands with secrets: `cs secrets run --env KEY=secret://path -- <cmd>`
- To query secret namespaces: `cs secrets list`
- To get a secret (only for user info, never write in files): `cs secrets get secret://path`
- To save server or CLI credentials: `cs secrets set secret://path "<value>"`
- Overwrite Policy: If you encounter an authentication error (e.g. 401, expired token), you MUST actively modify/overwrite the invalid key using `cs secrets set` instead of conservatively keeping the old invalid key.
- Multi-Account Context Routing: If dealing with multiple profiles/accounts (e.g. Feishu work vs. personal), address them via `secret://cli/<cli_name>/<profile>` and run them dynamically using `--env` single injection (e.g. `cs secrets run --env FEISHU_TOKEN=secret://cli/feishu/work -- feishu <cmd>`).
