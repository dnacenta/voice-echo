# Release

Create a release following the project's release flow.

## Steps

1. **Determine next version**: Check the current version in `Cargo.toml` and review commits since the last release tag to determine the appropriate semver bump (patch/minor/major). Ask the user to confirm the version.

2. **Create release branch**: Branch from `development` as `release/vX.Y.Z` (no rc suffix).

3. **Bump version**: Update `version` in `Cargo.toml` and regenerate `Cargo.lock` with `cargo generate-lockfile`.

4. **Commit**: `chore: bump version to vX.Y.Z` — no Co-Authored-By line.

5. **Push**: Push the release branch with `-u`.

6. **Create PR to main**:
   - Title: `release: vX.Y.Z`
   - Body: summary of commits since last release

7. **Create PR to development** (backmerge):
   - Title: `release: vX.Y.Z to development`
   - Body: `Backmerge release vX.Y.Z — carries version bump to development.`

8. **Report**: Print both PR URLs and remind that main should be merged first.

## Notes

- Merge strategy is merge commit (not squash)
- An automated workflow creates the git tag and GitHub release when the main PR is merged
- Always merge main PR first, then development PR
