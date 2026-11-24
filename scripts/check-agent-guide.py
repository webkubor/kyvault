"""Verify the passive Kyvault guide's shipped entrypoints without running an agent."""

from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
SHARED = ROOT / ".agents/skills/kyvault/SKILL.md"
CLAUDE = ROOT / ".claude/skills/kyvault/SKILL.md"
OPENCODE = ROOT / ".opencode/commands/kyvault.md"


def main() -> None:
    shared, claude, opencode = (
        path.read_text(encoding="utf-8") for path in (SHARED, CLAUDE, OPENCODE)
    )

    assert shared.startswith("---\nname: kyvault\n")
    assert "A bare invocation is help only" in shared
    assert "secret://example/demo -- trusted-command" in shared
    assert "disable-model-invocation: true" in claude
    match = re.search(r"\]\(([^)]+)\)", claude)
    assert match is not None and (CLAUDE.parent / match.group(1)).resolve() == SHARED
    assert 'local skill named `kyvault`' in opencode
    assert "do not run commands" in opencode
    policy = (SHARED.parent / "agents/openai.yaml").read_text(encoding="utf-8")
    assert "allow_implicit_invocation: false" in policy
    for content in (shared, claude, opencode):
        assert not re.search(r"(?m)^\s*(allowed-tools:|hooks:|!`)", content)

    local_only = [
        ".agents/skills/kyvault/fixture-local.env",
        ".claude/skills/kyvault/fixture-cache",
        ".claude/worktrees/fixture",
        ".opencode/fixture-settings.json",
        ".opencode/commands/fixture-private.md",
    ]
    ignored = subprocess.run(
        ["git", "check-ignore", "--", *local_only],
        cwd=ROOT,
        check=True,
        text=True,
        capture_output=True,
    )
    assert ignored.stdout.splitlines() == local_only
    print("Kyvault guide contracts passed (no agent, network, or vault accessed).")


if __name__ == "__main__":
    main()
