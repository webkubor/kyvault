# Awesome Rust — draft only, not submitted

Recommended first list target. Checked 2026-10-20 using GitHub REST API; upstream pushed 2026-10-20, not archived. Kyvault had 55 stars at inspection time. Recheck eligibility and neighbouring lines before submission.

Target: [rust-unofficial/awesome-rust](https://github.com/rust-unofficial/awesome-rust), `README.md`, `## Applications` → `### Security tools`. Snapshot README blob: `459fbf1398f774da417f966c421d12c8136c525c`.

Sort by the displayed ACCOUNT/REPO key, not by the product name. `webkubor/kyvault` sorts after `timescale/rsigma`, the current final entry, before `### Social networks`. Do not also add the CLI to Libraries → Cryptography.

## Actual contribution requirements

Source: [CONTRIBUTING.md](https://github.com/rust-unofficial/awesome-rust/blob/main/CONTRIBUTING.md).

> - Accepted: `(stars > 50 | downloads > 2000)`
> - Template: `[ACCOUNT/REPO](https://github.com/ACCOUNT/REPO) [[CRATE](https://crates.io/crates/CRATE)] - DESCRIPTION`
> - Sort: alphabetical

> if you've not published your crate to `crates.io` remove the `[[CRATE](...)]` part.

> if you have a CI build, please add the build badge. Put the image after the description, separated by a space. Please make sure to add the branch information to the image

> please pay attention to the alphabetical ordering

The [PR template](https://github.com/rust-unofficial/awesome-rust/blob/main/.github/pull_request_template.md) contains one checkbox asserting the contribution criteria were read. The guidelines describe editing through GitHub's pen icon; they do not explicitly require a particular fork workflow. No claim of a mandatory fork rule is made here.

Registry API returned “crate kyvault does not exist” during inspection; another agent owns registry work, so recheck before adding a crate link. This draft intentionally omits that link.

## Exact one-line entry

```markdown
* [webkubor/kyvault](https://github.com/webkubor/kyvault) - Local-first encrypted secret and API key manager with subprocess environment injection [![Test](https://github.com/webkubor/kyvault/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/webkubor/kyvault/actions/workflows/test.yml)
```

## Patch in actual neighbouring context

The three unchanged entries below are copied from the API-fetched README, not reconstructed:

```diff
 * [sherlock](https://github.com/jonaylor89/sherlock-rs) [[sherlock](https://crates.io/crates/sherlock)] - Hunt down social media accounts by username across social networks [![status](https://github.com/jonaylor89/sherlock-rs/actions/workflows/rust.yml/badge.svg)](https://github.com/jonaylor89/sherlock-rs/actions/workflows/rust.yml)
 * [ssh-vault](https://github.com/ssh-vault/ssh-vault) - A simple tool to manage secrets using ssh keys for encryption and decryption.
 * [timescale/rsigma](https://github.com/timescale/rsigma) [[rsigma](https://crates.io/crates/rsigma)] - A complete detection engineering toolkit for the Sigma detection standard, with a parser, evaluation engine, rule conversion, streaming runtime, linter, CLI, MCP, and LSP [![CI](https://github.com/timescale/rsigma/actions/workflows/ci.yml/badge.svg)](https://github.com/timescale/rsigma/actions/workflows/ci.yml)
+* [webkubor/kyvault](https://github.com/webkubor/kyvault) - Local-first encrypted secret and API key manager with subprocess environment injection [![Test](https://github.com/webkubor/kyvault/actions/workflows/test.yml/badge.svg?branch=main)](https://github.com/webkubor/kyvault/actions/workflows/test.yml)
 
 ### Social networks
```

The section uses `*`, account/repo labels, a single sentence after ` - `, and often a trailing CI badge. This entry follows those conventions.

## Ready-to-paste PR description

Title: Add webkubor/kyvault to Security tools

Add kyvault, a Rust CLI for local encrypted secret/API-key storage and subprocess environment injection, under Applications → Security tools.

The project has 55 GitHub stars at preparation time, meeting the documented popularity threshold. The entry follows ACCOUNT/REPO ordering and includes the existing Test workflow badge with `branch=main`. It omits a crate link pending registry availability.

Repository: https://github.com/webkubor/kyvault

## Claim evidence

- Rust CLI, package version and MIT: [Cargo manifest](../../rust/Cargo.toml).
- AES-256-GCM local encryption: [crypto implementation](../../rust/src/crypto.rs), functions `encrypt_joined` and `decrypt_joined`.
- Subprocess injection: [CLI implementation](../../rust/src/main.rs), `Cmd::Run`, lines 1081–1099 at inspection; resolves references and calls `cmd.env(name, v)`.
- Workflow/badge target: [Test workflow](../../.github/workflows/test.yml).

The feature wording does not promise that a subprocess cannot print a secret or that a model with permission to call `ky get` cannot obtain plaintext.
