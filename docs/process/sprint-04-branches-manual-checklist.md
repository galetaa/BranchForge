# Sprint 04 - Branches Manual Regression Checklist

Use this checklist to validate branch workflow behavior after Sprint 04 changes.

## Setup

1. Open a repository with at least one commit on the current branch.
2. Ensure there are no ongoing merge/rebase/cherry-pick operations.
3. Ensure the working tree is clean before checkout tests.

## Checklist

- Branches panel shows local branches and marks the current branch.
- Current branch line is visible in status, history, and branches views.
- Create branch adds a new branch and keeps UI state consistent.
- Checkout branch updates current branch in UI and refreshes status/history/diff context.
- Delete branch blocks deleting the currently checked out branch with a clear message.
- Dirty working tree blocks checkout with a clear, user-facing error.

## Notes

Record failures with exact steps, branch names, and relevant logs.

