<!-- A PR that changes behaviour should reference an issue. -->

Closes #

## What this changes

## Verification

- [ ] `bun run test` passes
- [ ] `bun run lint` passes (warnings are failures)
- [ ] `bun run check:build` passes, if any `.vue` file changed
- [ ] `bun run test:rust` and `bun run test:swift` pass, if `src-tauri/` or `sidecar/` changed (after `bun run sidecar`)
- [ ] Each new or changed test was mutation-checked: I broke the code it guards and watched it fail. Evidence:
- [ ] The README's "Using the app" manual is updated in this PR, if user-facing behaviour changed

**If this touches photo analysis or the sidecar**, say what it was run on:

- [ ] Real photos (formats and count):
- [ ] `sidecar/Fixtures` only

**Privacy:** no pixels or image files are sent anywhere; anything leaving the
Mac is derived JSON.
