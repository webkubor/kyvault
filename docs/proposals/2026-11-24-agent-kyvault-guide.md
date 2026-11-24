# Plan: passive Kyvault guide for four agent hosts

1. Keep the existing operations skill and vault implementation separate. Create a portable, instruction-only `kyvault` skill with safe defaults, no scripts/hooks/tool preapprovals, no vault actions at activation, and the short shell command `ky` explained.
2. Expose that guide from the repository to Antigravity CLI, Claude Code, Codex and OpenCode via each host's documented loader. Never fake unsupported bare slash aliases or silently install into existing profiles.
3. Check native registration wherever a documented read-only way exists using isolated client configuration, and test passivity, adapter references and invocation contracts without authenticating, starting a model or accessing a real vault.
