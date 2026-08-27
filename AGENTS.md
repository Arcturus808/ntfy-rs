## Commit Message Rules

- Do NOT add any AI attribution footers to commit messages
- Do NOT add "Generated with [Devin]" or similar watermarks
- Do NOT add "Co-Authored-By" lines for AI agents or bots
- Commit messages must be written as if authored solely by the human developer
- Keep commit messages concise: a short subject line, optional body, nothing else

## Force Push Policy

- The `main` branch is protected by a repository ruleset (block force pushes, restrict deletions)
- Repository admin has bypass permission — force pushes will succeed but must be intentional
- NEVER force push to `main` without explicitly telling the user what you are about to do and why
- If a force push is needed to remove confidential info from history, confirm the plan with the user first
- Prefer `git push --force-with-lease` over `git push --force` to avoid overwriting unexpected remote changes

## CI Credit Conservation

GitHub Actions free tier is limited. The CI workflow (`ci.yml`) runs on every push to `main` and every PR. The release workflow (`release.yml`) runs on version tags and builds 5 platform binaries (Windows, 2x Linux, 2x macOS).

**Rules:**
- **Default: add `[skip ci]` to ALL commit and merge messages** — CI is only run deliberately, not on every push
- **Remove `[skip ci]` only when preparing for a release** or when you need CI to verify a significant change
- **Run checks locally before pushing** to avoid wasted CI minutes on broken builds:
  ```
  cargo check
  cargo clippy --all-targets -- -D warnings
  cargo test
  ```
- **Batch commits when possible** — one push with multiple commits is one CI run
- **Do NOT push tags casually** — each tag triggers the full 5-matrix release build

## MSRV Policy

The declared MSRV is 1.85 (set in `Cargo.toml` via `rust-version`). This is driven by `lettre 0.11.22` and `clap 4.6.1`, both of which require Rust 1.85. Both are behind feature flags that are on by default.

**Rules:**
- Before using a stdlib API, verify it is stable under the declared MSRV (1.85). CI running on `stable` does NOT guarantee the API exists on 1.85.
- When bumping the MSRV, update ALL of these files in the same commit:
  1. `Cargo.toml` — `rust-version` field
  2. `README.md` — build prerequisites section
  3. `AGENTS.md` — this section
- If a dependency update raises the effective MSRV, update `rust-version` to match
- For minimal embedded builds (`default-features = false`), the effective MSRV is lower (~1.71, driven by tokio and aws-lc-rs), but the declared MSRV should reflect the default feature set

## Branching Rules

- **All work goes on its own branch** — never commit directly to `main`. Use a descriptive branch name prefixed by type: `fix/...`, `feat/...`, `refactor/...`, `docs/...`, `chore/...`, `release/...`.
- **Branch from `main`** and merge back with `--no-ff` to preserve branch history.
- **Run pre-commit checks** (`cargo check`, `cargo clippy`, `cargo test`) on the branch before merging build-affecting changes.
- **Delete the branch after merging to `main`**.
- **Merge commit messages** follow the pattern: `Merge <branch-name>: <short description> [skip ci]` (always include `[skip ci]` unless explicitly preparing for a release).
- **Branch pushes don't trigger CI** — only pushes to `main` and PRs against `main` trigger `ci.yml`.

## Pre-commit Checks

Before pushing or merging build-affecting changes, run locally:

| Check | Command |
|---|---|
| Compile check | `cargo check` |
| Lint | `cargo clippy --all-targets -- -D warnings` |
| Tests | `cargo test` |
| Linux verification (before releases) | Run via WSL: `cargo test` and `cargo clippy` on Ubuntu |

For Linux verification via WSL (one-time setup):
```bash
wsl -d Ubuntu -u root -- apt install -y build-essential pkg-config
wsl -d Ubuntu -- curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

WSL checks (run from the project root):
```bash
wsl -d Ubuntu -- bash -c "source ~/.cargo/env && cd /mnt/<your-repo-path> && cargo test && cargo clippy --all-targets -- -D warnings"
```

**Pre-commit checklist:**
1. Does the change affect compiled code, tests, or build config? (Rust source, Cargo.toml, CI config, test files)
2. If YES → verify locally (check, clippy, test) before committing. For Linux-specific issues, also run WSL checks.
3. If NO → no local verification needed.
4. **Always add `[skip ci]` to the commit message** — regardless of whether the change is build-affecting or not. CI is only run deliberately during release prep.