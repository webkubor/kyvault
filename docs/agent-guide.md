# Kyvault in agent tools

`ky` is the shell shorthand for `kyvault`. The agent entry points below load **usage help only**. They do not access a vault, install the CLI, run a credential operation, or grant permissions. This is separate from `kyvault connect` and the existing `kyvault-ops` instructions.

Open this checkout as the workspace in your agent and start a new session if its customization list was cached.

| Agent | Type into the agent | Workspace source |
| --- | --- | --- |
| [Antigravity CLI (`agy`)](https://antigravity.google/docs/skills) | `/kyvault` | `.agents/skills/kyvault/SKILL.md` |
| [Claude Code](https://code.claude.com/docs/en/skills) | `/kyvault` | `.claude/skills/kyvault/SKILL.md` forwards to the shared guide |
| [Codex](https://learn.chatgpt.com/docs/build-skills) | `$kyvault`, or select it from `/skills` | `.agents/skills/kyvault/SKILL.md` |
| [OpenCode](https://opencode.ai/docs/commands/) | `/kyvault` | `.opencode/commands/kyvault.md` loads the shared guide |

Codex has no documented bare `/kyvault` custom-command registration through this mechanism; use the native skill mention instead. Do not add legacy custom prompts merely to imitate the spelling. Antigravity CLI can run `/skills reload` if this guide is added while it is already open.

## Use it in another workspace

Copy the entire `.agents/skills/kyvault/` directory into that project's `.agents/skills/`. Also copy the Claude skill wrapper or OpenCode command shown above if you use either client. Preserve their repository-relative paths; skip the copy if the target already has a custom `kyvault` entry until you have reviewed the conflict. This is an opt-in file copy, not a global installation or change to `kyvault connect`.

For a machine-wide personal shortcut across your workspaces, you can instead copy the same skill into the host's personal location. Don't overwrite an existing same-name customization.

- **agy:** copy the shared skill folder to `~/.gemini/config/skills/kyvault/`.
- **Claude Code:** copy the shared `SKILL.md` itself to `~/.claude/skills/kyvault/SKILL.md` (not the workspace-only wrapper). To keep personal activation manual only, add `disable-model-invocation: true` to the copy's YAML frontmatter.
- **Codex:** copy the shared skill folder to `~/.agents/skills/kyvault/`, including its invocation policy. You may see duplicate entries when the same skill is also available in the project; select the intended copy.
- **OpenCode:** copy the shared `SKILL.md` to `~/.config/opencode/skills/kyvault/SKILL.md` and the command to `~/.config/opencode/commands/kyvault.md`.

This does not install or open the Kyvault executable. Any actual credential use remains separately authorized and must happen on a machine with permitted access.

Maintainers can run `python3 scripts/check-agent-guide.py` to verify the bundled entrypoints and private-file exclusions without launching a client or opening a vault.
