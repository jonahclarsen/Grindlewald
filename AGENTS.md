# Repository instructions

- Never publish secrets, credentials, private device identifiers, personal paths, or local runtime data. Before every push, inspect both the staged diff and repository history for sensitive values.
- Always commit and push any changes the user asked for immediately after implementing and verifying them.
- Prefer `pnpm` over `npm`.
- Keep device configuration in the user's Application Support directory, never in this repository.
