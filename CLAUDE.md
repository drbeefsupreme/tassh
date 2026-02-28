# Claude Software Factory — Claude Instructions

tassh is a Rust CLI tool. Use these commands for development.

## Build & Test

- Build: `cargo build`
- Test: `cargo test`
- Lint: `cargo clippy --all-targets --all-features -- -D warnings`
- Format check: `cargo fmt --check`
- Format fix: `cargo fmt`

Always run clippy and format before committing. Ensure CI passes.

## Pull Requests

Always create PRs using `gh pr create`. Never substitute a compare link.

```
gh pr create \
  --repo OWNER/REPO \
  --title "..." \
  --body "..." \
  --base master \
  --head <branch>
```

The PR `--body` must include `Closes #<issue-number>` so GitHub auto-closes the originating
issue on merge. Example body footer:

```
Closes #42
```

After creating a PR, apply the `claude-task` label so the stale workflow does not auto-close it:

```
gh issue edit <PR-number> --repo OWNER/REPO --add-label claude-task
```

(GitHub treats PRs as issues for the label API, so `gh issue edit` works on PR numbers.)

Always run this as the final step. The PR must exist before marking the task complete.

## Issues

When creating a GitHub issue, always include `@claude` at the end of the body so the
workflow auto-triggers. Example closing line:

```
@claude please implement this
```

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/):

- `feat:` — new feature (bumps MINOR version)
- `fix:` — bug fix (bumps PATCH version)
- `feat!:` or `BREAKING CHANGE` — breaking change (bumps MAJOR version)
- `chore:`, `docs:`, `test:`, `refactor:` — no version bump

## Branch Naming

`claude/issue-{number}-{YYYYMMDD}-{HHMM}`
