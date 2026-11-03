# Repository instructions

- Do all work in a separate Git worktree. Only bring completed, verified changes into this main checkout once the work is done, then commit and push as required below. The user may explicitly waive this workflow for a task.
- Never publish secrets, credentials, private device identifiers, personal paths, or local runtime data. Before every push, inspect both the staged diff and repository history for sensitive values.
- Always commit and push any changes the user asked for immediately after implementing and verifying them.
- Prefer `pnpm` over `npm`.
- Keep device configuration in the user's Application Support directory, never in this repository.
- Before changing the light protocol or planning new light features, read `docs/govee-app-research.md`. It holds what we learned from the Govee app, how to get and decompile it again, what is verified, and the list of future ideas. Add new findings there. Never commit an APK or decompiled source.
- Test on real lights as that document's "Testing on real lights" section describes.
- Android builds and releases run on CI, as the README's "Android and sync" section describes. `android.yml` builds and emulator-tests every push to main. `nightly-release.yml` publishes a signed APK once a day when main has a new CI-verified commit; GitHub often starts it hours after its scheduled time. Run it by hand to release sooner.
