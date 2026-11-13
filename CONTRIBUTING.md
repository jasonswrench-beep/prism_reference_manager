# Contributing

Thank you for helping. A few rules keep the project dependable.

## Terms

Contributions are accepted under the Apache License 2.0, the project's license: by submitting
a contribution you agree it may be distributed under that license (inbound equals outbound, as
section 5 of the license describes). Add a `Signed-off-by:` line to each commit
(`git commit -s`) to confirm you wrote the change or have the right to submit it.

## Before you start

- For anything larger than a small fix, open an issue first so the approach can be agreed.
- Security problems go through private reporting, not issues (see SECURITY.md).

## Writing changes

- Keep the program local-first. Any new outbound request must be optional, listed in the
  network inventory with what it sends, follow the network settings and be logged.
- Never change citation output without updating the golden cases. A changed expected output
  needs review by a qualified person before the case counts as reviewed.
- Records, notes and annotations are never changed silently. Batch changes are proposals the
  person reviews, and every write is one undoable step with provenance.
- Interface text is plain language. Code comments, commit messages, documentation and release
  notes describe features generically and do not compare the program with others.
- Accessibility is part of done: keyboard paths, labels and visible focus for every control.

## Checks

```
cargo test -p prm-core
node --check app/ui/js/*.js app/ui/js/views/*.js
python tools/third_party.py
```

The window can be exercised end to end in a debug build with `PRM_TEST_SCRIPT`,
`PRM_TEST_LOG` and `PRM_CONFIG_DIR` (see README). New dependencies must use a license on the
allow-list in `tools/third_party.py`.
