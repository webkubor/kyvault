# Outreach preparation — not published

Research snapshot: 2026-10-20. Scope was reduced by the lead to this short index and two useful drafts while scorecard work takes priority. No posts, comments, issues, PRs, registry publication, commits, or pushes were made by this task. All task artifacts are in this directory.

Repo path was derived from `git remote get-url origin` with the requested sed pattern: [webkubor/kyvault](https://github.com/webkubor/kyvault). Owner spelling was not assumed.

## Artifacts and order

| Order | Target | Artifact | What it is | Effort | Risk / next action |
|---|---|---|---|---|---|
| 1 | Awesome Rust | [awesome-rust.md](awesome-rust.md) | Exact section, neighbour context, entry and PR description | Low | Owner/lead rechecks 50-star threshold, duplicate suggestions, current context and crate availability; GitHub login required to submit |
| 2 | Hacker News | [launch-posts.md](launch-posts.md) | Show HN title, 3 alternatives, introductory comment, evidence | Medium | Owner must be present for discussion; HN login required; no absolute secrecy claims |
| — | Handoff | [README.md](README.md) | Target triage, source map, limitations | Low | Internal preparation only |

Do the factual wording review before either submission. One relevant list PR and one discussion are useful; spraying identical copy across communities is likely to be read as promotion spam. Package-registry work belongs to the distribution agent; no package publication was attempted here.

## Lists actually checked

Maintenance here means repository metadata / observed changes, not a guarantee of a maintainer response.

| Repository | Actual location / fit | Activity checked | Recommendation |
|---|---|---|---|
| rust-unofficial/awesome-rust | `README.md` → Applications → Security tools; `webkubor/kyvault` follows `timescale/rsigma` at the end | Pushed 2026-10-20, not archived | Strongest verified fit; draft prepared |
| awesome-selfhosted/awesome-selfhosted | Main PR template redirects to `awesome-selfhosted/awesome-selfhosted-data`; data tag `Password Managers`, potential `software/kyvault.yml` | Main pushed Oct18; data repo Oct20 | Skip now: first release Jun25 is under four months old on Oct20; CLI scope fit is uncertain. Reassess after Oct25, with strict “more than” wording preferably Oct26 |
| awesome-foss/awesome-sysadmin | Main PR template redirects to `awesome-foss/awesome-sysadmin-data`; no secrets category; `Miscellaneous` would be weak | Main pushed Oct15; data Oct18 | Skip: weak category fit; don't force into Automation just because OpenBao appears there |
| sbilly/awesome-security | `README.md` → `## Datastores`; rules require bottom insertion after LunaSec, not alphabetical | Last push/observed merge Jan11; latest 100 closed PRs showed zero merges after Feb1, despite newer unmerged closures | Stale for additions; deprioritize. It has a plausible section but no ready draft retained |
| awesome-security/awesome-security | Empty-looking namesake, not the established sbilly list | Last push 2016-06-08, 0 stars | Skip inactive namesake |
| Puliczek/awesome-list-of-secrets-in-environment-variables | Index of environment variable names, not secret-manager tools | Last push 2022-09-21 | Skip: inactive and wrong content type |
| ishandutta2007/Awesome-Secrets-Credential-Management | Small credential list; section and complete contribution rules not assessed | Pushed Oct6, 1 star | Low reach; not a validated target |
| punkpeye/awesome-mcp-servers | MCP servers | Pushed Oct18 | Skip: kyvault source inspected here exposes a CLI, no verified MCP server |
| hesreallyhim/awesome-claude-code | Generated resource catalog; contribution guidelines use recommendation issues, not a routine README PR | Pushed Oct20 | Potential later research only; >=14 days plus active development OR >=100 stars; one recommendation at a time. No exact entry location verified |
| e2b-dev/awesome-ai-agents | Catalog of agents; no secrets/credential section found in fetched README | Pushed Aug21 | Skip: kyvault is a tool used by agents, not an agent |
| sindresorhus/awesome | Directory of awesome lists | Pushed Sep2; description says PRs temporarily disabled | Skip: individual tool is wrong scope |

The guessed candidate paths `rust-lang/awesome-rust`, `awesome-ai-agents/awesome-ai-agents`, `wphan/awesome-mcp`, `desireevl/awesome-vpn`, and `awesome-cryptography/awesome-cryptography` returned 404. A 404 for a guessed path does not establish that no such category list exists. VPN has no product fit; cryptography-library sections have no stronger fit than Rust Security tools. Awesome-devops and Chinese equivalents were not validated to file/section level; no target claims or drafts are made for them.

Sources: GitHub REST `repos`, `contents`, `search/repositories`, `releases`, and read-only `pulls?state=closed` requests. During research, a few gh calls with `-f` but no explicit GET returned validation/404 errors because gh inferred POST; no external write succeeded. Searches with literal plus signs were discarded; only correctly spaced/direct repository checks underpin the table.

## Contribution constraints worth preserving

[Selfhosted main PR template](https://github.com/awesome-selfhosted/awesome-selfhosted/blob/master/.github/PULL_REQUEST_TEMPLATE.md): “Please do not submit pull requests in this repository. Use https://github.com/awesome-selfhosted/awesome-selfhosted-data instead.”

[Selfhosted-data contributing](https://github.com/awesome-selfhosted/awesome-selfhosted-data/blob/master/CONTRIBUTING.md) explicitly says “Write the text of an entry ... that a person will then submit as their own” is not permitted, and says agents may explain the guidelines and check objective requirements. No selfhosted submission text was created. The [addition template](https://github.com/awesome-selfhosted/awesome-selfhosted-data/blob/master/.github/ISSUE_TEMPLATE/addition.md) requires “Any software project you are adding was first released more than 4 months ago.” The first GitHub release was v1.0.0, 2026-06-25.

[Security contribution rules](https://github.com/sbilly/awesome-security/blob/master/contributing.md): “Link additions should be added to the bottom of the relevant category.” Therefore an alphabetical insertion there would violate the actual rule.

[Claude Code guidelines](https://github.com/hesreallyhim/awesome-claude-code/blob/main/CONTRIBUTING.md): “You may not recommend more than one resource at a time.” They also warn there is no guaranteed response or formal submission/review contract. Don't treat inclusion as an entitlement.

## Factual corrections and traceability

| Claim / brief discrepancy | Evidence read | Wording to use |
|---|---|---|
| Rust CLI, v2.6.0, MIT | [manifest](../../rust/Cargo.toml), [SKILL](../../SKILL.md), [license](../../LICENSE) | Verified at inspection; refresh version if another agent releases |
| AES-256-GCM local encryption | [crypto](../../rust/src/crypto.rs), `encrypt_joined` / `decrypt_joined` | File backend encrypts/decrypts locally |
| “zero network dependency” for entire product | [CLI](../../rust/src/main.rs), `Backend::select`; [D1](../../rust/src/d1.rs), [providers](../../rust/src/providers.rs), [update](../../rust/src/update.rs), [GitLab](../../rust/src/gitlab.rs) | Local file decryption works offline; optional D1 / sync / checks / updates use network |
| “AI never sees plaintext” / inevitable training-data exposure | [CLI](../../rust/src/main.rs), `Cmd::Get` prints plaintext; `Cmd::Run` uses child env and inherited output | Reference-based injection avoids handing values to model context for normal use; no sandbox or provider-training guarantee |
| `ky import env --preview` | [CLI](../../rust/src/main.rs), `Cmd::Import`; [import implementation](../../rust/src/import_env.rs) | Actual syntax: `ky import --file .env --dry-run` |
| `ky ui`, `ky wizard`, URI / aliases | [UI](../../rust/src/ui.rs), [wizard](../../rust/src/wizard.rs), [aliases](../../rust/src/alias.rs), [SKILL](../../SKILL.md) | Verified; UI binds loopback |
| `ky connect` to DSH | [connect](../../rust/src/connect.rs) | Gemini/agy, Claude, Codex, Hermes and project rule files verified; no dedicated DSH branch in source |
| Server ledger and CLI profiles | [CLI](../../rust/src/main.rs), [store](../../rust/src/store.rs), [SKILL](../../SKILL.md) | File-backend capability; not used as a headline in retained drafts |
| Auth Guard SSH + TOTP | [Auth Guard](../../rust/src/auth_guard.rs), [README](../../README.md) | Optional setup encrypts master key; default any_one SSH unlock, TOTP is for sensitive reveals, not mandatory MFA on every run |
| Prebuilt binaries and ky alias | [installer](../../install.sh), [release workflow](../../.github/workflows/release.yml) | No Rust/Python/Node runtime needed for downloaded binaries; avoid “fully static / no system dependencies” because Linux targets are GNU, not musl |
| Validity and balance checking | [providers](../../rust/src/providers.rs), [CLI](../../rust/src/main.rs) | Provider-dependent validity/balance checks, which use network |

Do not repeat README's “absolute safety” or “military grade” language. The SSH-derived protection uses public-key material; this research does not establish private-key-possession authentication or resistance to a locally privileged reader.

## Deferred platform work and research limits

The requested seven Chinese platform posts, blog title set, four Reddit posts and Product Hunt note were paused by the lead's scope reduction. They are not completed artifacts.

Two real Juejin articles were read for future style reference: [Node CLI walkthrough](https://juejin.cn/post/6924498607655616520) and [AI Agent API key handling](https://juejin.cn/post/7520204324770463785). Both use subsection headings and concrete code/config examples; the latter's hype tone should not be imitated. This is not 2–3 samples per platform. No claim is made that Zhihu/CSDN/Jianshu/SSPai/V2EX/SegmentFault conventions were verified.

Reddit rules endpoints and alternate reads were blocked (403 / HTML challenge); no subreddit self-promotion rules were verified, so no Reddit draft is marked ready. HN's official Show HN guidelines were readable and are linked in its draft. Owner login would be needed for any eventual platform posting; no login action was requested here.
