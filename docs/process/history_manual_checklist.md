# History Manual Checklist

Use this checklist to validate history UX behavior.

## Setup

1. Open a repo with at least 3 commits.
2. Ensure there are no ongoing merge/rebase operations.

## Checklist

- History panel loads and shows recent commits.
- Selecting a commit updates the details pane.
- Load more (paging) appends commits without losing selection.
- Commit details match the selected commit.
- History errors are shown as a clear message, not raw stderr.

## Notes

Record any discrepancies or unexpected selection resets.
