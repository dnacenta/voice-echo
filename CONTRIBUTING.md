# Contributing

Contributions are welcome. This document explains the workflow.

## Reporting bugs or requesting features

Open an [issue](https://github.com/dnacenta/trinity-echo/issues). Use a clear title and include enough context to reproduce the problem or understand the request.

## Making changes

1. Fork the repo
2. Create a branch from `development` (see naming below)
3. Make your changes
4. Open a PR targeting `development`

`main` is protected. All changes go through `development` first.

## Branch naming

Branches follow this pattern:

```
<type>/<issue-number>-<short-description>
```

| Type       | When to use                          | Example                              |
|------------|--------------------------------------|--------------------------------------|
| `feat`     | New functionality                    | `feat/12-outbound-sms`               |
| `fix`      | Bug fix                              | `fix/7-vad-false-trigger`            |
| `refactor` | Code restructure, no behavior change | `refactor/15-extract-audio-pipeline` |
| `docs`     | Documentation only                   | `docs/3-config-reference`            |
| `chore`    | Maintenance, deps, CI                | `chore/20-update-dependencies`       |

If there's no issue yet, create one first so there's a number to reference.

## Commit messages

This project uses [Conventional Commits](https://www.conventionalcommits.org/) (lowercase):

```
<type>(<scope>): <description>
```

Examples:

```
fix(vad): prevent false trigger on background noise
feat(api): add sms endpoint
docs: add configuration examples
refactor(tts): split client into streaming and batch
```

Rules:
- Lowercase everything
- Imperative, present tense ("add" not "added")
- No period at the end
- Reference the issue in the body or footer: `Closes #7`

## Pull request titles

PR titles follow the same convention, referencing the issue number as scope:

```
fix(#7): prevent vad false trigger on background noise
feat(#12): add outbound sms support
docs(#3): expand configuration reference
```

## Code style

- Run `cargo clippy` before submitting -- no warnings
- Run `cargo fmt` for formatting
- Run `cargo test` to make sure nothing breaks
- Keep changes focused -- one issue per PR
