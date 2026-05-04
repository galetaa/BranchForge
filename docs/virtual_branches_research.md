# Virtual Branches Research Prototype

Virtual branches are research-only in this beta. BranchForge records logical contexts for changed paths without checking out a Git branch or mutating refs.

## Commands

- `virtual.detect [name] [path...]`: map current staged, unstaged, and untracked paths into a logical context.
- `virtual.create <name> <path...>`: create a named logical context from explicit paths.
- `virtual.switch [virtual_id|name]`: mark a context active in UI state only.
- `virtual.export_patch [virtual_id|name]`: render staged and unstaged patch text for the context.

## Current Limits

- Switching context does not hide, apply, or shelve files.
- Untracked files are listed in exports but not embedded as patch bodies.
- The prototype is intended to validate UX and data modeling before a patch/index abstraction is promoted into the beta workflow.
