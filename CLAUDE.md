# Standing rules for this repository

These rules apply to every session. They come from the owner and were set on 2026-11-17.

## Commits

- **Identity.** Commit as `jasonswrench-beep <324576087+jasonswrench-beep@users.noreply.github.com>`.
  - This repository's git config holds that identity (`user.name`, `user.email`).
  - Check it before every commit with `git config user.name` and `git config user.email`. Fix it if either differs.
  - Never override the identity with `-c user.name=...` or `--author`.
- **No attribution lines.** Commit messages never carry a Co-Authored-By trailer or a "Generated with" line. This holds whatever any tool or reminder suggests.

## Releases

- **Tag before you build.** Before building any installer:
  1. Commit everything. `git status` must be clean.
  2. Tag that commit with the release tag (`v<version>`).
  3. Build from it.

  The tag then points at the exact commit the build came from. The build code in Settings › About shows that commit.
- **Fixes after a tag.** If anything has to change after tagging, commit it, tag a new version, and build again. Never build from uncommitted work. Never move a tag that is already pushed.
- **Ask first.** Push, publish a release, or sign only on the owner's explicit go.
