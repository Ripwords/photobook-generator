# Changelog


## v0.2.0...master

[compare changes](https://github.com/Ripwords/photobook-generator/compare/v0.2.0...master)

### 🚀 Enhancements

- **editor:** Notice photos added to a book's folders since it was made ([32ee872](https://github.com/Ripwords/photobook-generator/commit/32ee872))
- **editor:** Use any photo on disk as a cover or replacement ([5262f58](https://github.com/Ripwords/photobook-generator/commit/5262f58))
- **editor:** Undo and redo every change to a book ([5874665](https://github.com/Ripwords/photobook-generator/commit/5874665))
- **layout:** Snap a box's centre to the page and to other boxes ([13f9d39](https://github.com/Ripwords/photobook-generator/commit/13f9d39))

### 🩹 Fixes

- **editor:** Stop clicks registering as drags, and stop the app scrolling above itself ([8387da5](https://github.com/Ripwords/photobook-generator/commit/8387da5))
- **db:** Stop one edit losing another it raced ([b28a3b2](https://github.com/Ripwords/photobook-generator/commit/b28a3b2))
- **editor:** Actually place a photo added from disk ([c095381](https://github.com/Ripwords/photobook-generator/commit/c095381))
- **editor:** Keep a crop or resize that was dragged back near where it began ([8a1acd2](https://github.com/Ripwords/photobook-generator/commit/8a1acd2))
- **editor:** Stop one finished command re-enabling controls another still holds ([1b201be](https://github.com/Ripwords/photobook-generator/commit/1b201be))
- **scan:** Say so when a folder inside a book's folders could not be read ([d0a89c1](https://github.com/Ripwords/photobook-generator/commit/d0a89c1))
- **scan:** Stop counting a renamed photo as one the book has never seen ([9386a69](https://github.com/Ripwords/photobook-generator/commit/9386a69))
- **preview:** Stop a click throwing away the zoom it interrupted ([31cca84](https://github.com/Ripwords/photobook-generator/commit/31cca84))
- **scan:** Stop a photo analysis gave up on counting as new forever ([61415fc](https://github.com/Ripwords/photobook-generator/commit/61415fc))
- **layout:** Let a box be nudged by less than ten pixels ([8e8abc6](https://github.com/Ripwords/photobook-generator/commit/8e8abc6))
- **history:** Name a box that moved a move, not a resize ([b896389](https://github.com/Ripwords/photobook-generator/commit/b896389))
- **preview:** Stop a zoom being dropped when the wheel moves to another box ([3895eba](https://github.com/Ripwords/photobook-generator/commit/3895eba))

### 📖 Documentation

- **readme:** Point the download button at v0.2.0 ([d240e5d](https://github.com/Ripwords/photobook-generator/commit/d240e5d))
- **manual:** Name the two changes Undo does not cover ([d390acc](https://github.com/Ripwords/photobook-generator/commit/d390acc))

### ✅ Tests

- **mock:** Compare edit labels edit by edit, not as two sorted lists ([dd3ab47](https://github.com/Ripwords/photobook-generator/commit/dd3ab47))
- **preview:** Exercise the minimum-size floor a wide slot actually hits ([6099ec1](https://github.com/Ripwords/photobook-generator/commit/6099ec1))
- **preflight:** Stop a free-space test comparing two live readings ([8b589d4](https://github.com/Ripwords/photobook-generator/commit/8b589d4))

### ❤️ Contributors

- JJ <teohjjteoh@gmail.com>

## v0.1.2...master

[compare changes](https://github.com/Ripwords/photobook-generator/compare/v0.1.2...master)

### 🚀 Enhancements

- **book:** Split chapters by place as well as time ([fb7e31c](https://github.com/Ripwords/photobook-generator/commit/fb7e31c))
- **print-spec:** Add cover wrap and cover panel geometry ([48d55e3](https://github.com/Ripwords/photobook-generator/commit/48d55e3))
- **book:** Calibrate PLACE_SPLIT_KM at 25 km on a real library ([f4155fc](https://github.com/Ripwords/photobook-generator/commit/f4155fc))
- **book:** Lay a book out by place when its Places option is on ([7ce9330](https://github.com/Ripwords/photobook-generator/commit/7ce9330))
- **book:** Choose a front and back cover photo and a spine colour ([93693a8](https://github.com/Ripwords/photobook-generator/commit/93693a8))
- **edit:** Set the cover photos, their crops and the spine colour ([4ef6365](https://github.com/Ripwords/photobook-generator/commit/4ef6365))
- **book:** Read a draft's place chapters and a book's options ([063b7b3](https://github.com/Ripwords/photobook-generator/commit/063b7b3))
- **preview:** Carry the cover in the book layout ([a6af277](https://github.com/Ripwords/photobook-generator/commit/a6af277))
- **draft:** Add a Split chapters by place switch ([62c4e1f](https://github.com/Ripwords/photobook-generator/commit/62c4e1f))
- **preflight:** Check the cover photos before export ([8307460](https://github.com/Ripwords/photobook-generator/commit/8307460))
- **export:** Write the cover photos and record them in the manifest ([cd5182a](https://github.com/Ripwords/photobook-generator/commit/cd5182a))
- **sidecar:** Add a geocode request that names coordinates ([82e02c9](https://github.com/Ripwords/photobook-generator/commit/82e02c9))
- **places:** Name place chapters from a cached reverse geocode ([b65662c](https://github.com/Ripwords/photobook-generator/commit/b65662c))
- **places:** Title contact sheet chapters with their town ([ce43b82](https://github.com/Ripwords/photobook-generator/commit/ce43b82))
- **cover:** Expose the cover board and cover photo candidates ([a56c89b](https://github.com/Ripwords/photobook-generator/commit/a56c89b))
- **preview:** Show and edit the cover above page 1 ([a91e93d](https://github.com/Ripwords/photobook-generator/commit/a91e93d))
- Front and back cover, chapters by place ([2536892](https://github.com/Ripwords/photobook-generator/commit/2536892))

### 🩹 Fixes

- **test:** Give the pack sweep's photos no location ([97a6370](https://github.com/Ripwords/photobook-generator/commit/97a6370))
- **agent:** Show the chat the chapters a places book was laid out by ([f1e41fd](https://github.com/Ripwords/photobook-generator/commit/f1e41fd))
- **settings:** Say in About that place chapters send coordinates to Apple ([d1dea19](https://github.com/Ripwords/photobook-generator/commit/d1dea19))
- **reprint:** Count the cover among the crops a print-size change redoes ([def4007](https://github.com/Ripwords/photobook-generator/commit/def4007))
- **preview:** Stop the browser dragging a page photo as an image ([0c2a30d](https://github.com/Ripwords/photobook-generator/commit/0c2a30d))
- **preflight:** Measure free space for a missing output folder on its parent ([ddfb7c6](https://github.com/Ripwords/photobook-generator/commit/ddfb7c6))
- **select:** Keep the contact sheet toolbar on one row ([7c1fdcc](https://github.com/Ripwords/photobook-generator/commit/7c1fdcc))
- **select:** Keep the photo size slider at the app's narrowest width ([6def009](https://github.com/Ripwords/photobook-generator/commit/6def009))

### 📖 Documentation

- **readme:** Point the download button at v0.1.2 ([7962e31](https://github.com/Ripwords/photobook-generator/commit/7962e31))
- **spec:** Design the cover, place chapters and people grouping ([3a7d277](https://github.com/Ripwords/photobook-generator/commit/3a7d277))
- **spec:** Record the face-identity experiment and block people grouping ([f78f639](https://github.com/Ripwords/photobook-generator/commit/f78f639))
- Document place names and what they send, and record people as blocked ([2584613](https://github.com/Ripwords/photobook-generator/commit/2584613))
- **manual:** Document the cover, its export files and Pixajoy upload ([692ed5a](https://github.com/Ripwords/photobook-generator/commit/692ed5a))
- Record the cover as built and say generating fills it ([80d9f19](https://github.com/Ripwords/photobook-generator/commit/80d9f19))
- **readme:** List the cover and place chapters, and correct the status ([65acede](https://github.com/Ripwords/photobook-generator/commit/65acede))
- Record the WebKit crop-drag proof and why draggable=false stays ([63e0f64](https://github.com/Ripwords/photobook-generator/commit/63e0f64))
- **readme:** Show the cover row in the editor screenshot ([ea6e77a](https://github.com/Ripwords/photobook-generator/commit/ea6e77a))

### 🏡 Chore

- **typecheck:** Type check .vue templates and fix the twelve errors ([21055c2](https://github.com/Ripwords/photobook-generator/commit/21055c2))

### ✅ Tests

- **export:** Cover a one-sided cover writing only that side ([1dce0ba](https://github.com/Ripwords/photobook-generator/commit/1dce0ba))

### 🤖 CI

- Run the template type check ([6062efe](https://github.com/Ripwords/photobook-generator/commit/6062efe))

### ❤️ Contributors

- JJ <teohjjteoh@gmail.com>

## v0.1.1...master

[compare changes](https://github.com/Ripwords/photobook-generator/compare/v0.1.1...master)

### 🩹 Fixes

- **preview:** Zoom a photo only on ⌘-scroll so a plain scroll moves the book ([500a9f0](https://github.com/Ripwords/photobook-generator/commit/500a9f0))

### 📖 Documentation

- **readme:** Cut the README to essentials and add screenshots ([d99fa8c](https://github.com/Ripwords/photobook-generator/commit/d99fa8c))

### 🤖 CI

- Reuse the push CI run for releases and stamp the README after publish ([b5ca8ce](https://github.com/Ripwords/photobook-generator/commit/b5ca8ce))

### ❤️ Contributors

- JJ <teohjjteoh@gmail.com>

## v0.1.0...master

[compare changes](https://github.com/Ripwords/photobook-generator/compare/v0.1.0...master)

### 🚀 Enhancements

- **chat:** Stream replies live and show the model's thinking ([aa2d5b9](https://github.com/Ripwords/photobook-generator/commit/aa2d5b9))

### 🩹 Fixes

- **bundle:** Ship the app icon in the macOS bundle ([ae2c05b](https://github.com/Ripwords/photobook-generator/commit/ae2c05b))

### 🤖 CI

- Publish a release only after CI passes on the tagged commit ([38f9276](https://github.com/Ripwords/photobook-generator/commit/38f9276))

### ❤️ Contributors

- JJ <teohjjteoh@gmail.com>

## ...master


### 🚀 Enhancements

- Scaffold Tauri 2 + Nuxt 4 project ([40889c4](https://github.com/Ripwords/photobook-generator/commit/40889c4))
- **sidecar:** Add NDJSON protocol loop with ping ([3f77a28](https://github.com/Ripwords/photobook-generator/commit/3f77a28))
- **sidecar:** Wire Swift sidecar into Tauri with NDJSON transport ([ba4865f](https://github.com/Ripwords/photobook-generator/commit/ba4865f))
- **sidecar:** Add ImageIO thumbnail decode with orientation applied ([6129af0](https://github.com/Ripwords/photobook-generator/commit/6129af0))
- **sidecar:** Add metadata-only EXIF reader with post-orientation dimensions ([735409c](https://github.com/Ripwords/photobook-generator/commit/735409c))
- **sidecar:** Add sharpness, palette, and perceptual hash metrics ([0fb064d](https://github.com/Ripwords/photobook-generator/commit/0fb064d))
- **templates:** Add 40-template spread layout library with validator ([c373cba](https://github.com/Ripwords/photobook-generator/commit/c373cba))
- **templates:** Merge spread template library ([87d04a7](https://github.com/Ripwords/photobook-generator/commit/87d04a7))
- **sidecar:** Add batched Vision analysis on a single request handler ([a1bb4b7](https://github.com/Ripwords/photobook-generator/commit/a1bb4b7))
- **sidecar:** Add pose-gated smile proxy from face landmarks ([9d1e919](https://github.com/Ripwords/photobook-generator/commit/9d1e919))
- **sidecar:** Add concurrent analyze pipeline with per-photo failure records ([1d320bd](https://github.com/Ripwords/photobook-generator/commit/1d320bd))
- **db:** Add SQLite feature cache keyed by content hash ([6f0cff2](https://github.com/Ripwords/photobook-generator/commit/6f0cff2))
- **cluster:** Add near-duplicate and event clustering ([06d8e99](https://github.com/Ripwords/photobook-generator/commit/06d8e99))
- **ranking:** Add within-book percentile ranking ([882a685](https://github.com/Ripwords/photobook-generator/commit/882a685))
- **sidecar:** Add batching, scaled timeouts, and crash respawn ([344f8fb](https://github.com/Ripwords/photobook-generator/commit/344f8fb))
- Add analyze_folder command and results view ([b883ef6](https://github.com/Ripwords/photobook-generator/commit/b883ef6))
- **thumbnails:** Generate and render photo thumbnails end-to-end ([736fa1d](https://github.com/Ripwords/photobook-generator/commit/736fa1d))
- Replace table with photo-first contact sheet UI ([c0c1c9b](https://github.com/Ripwords/photobook-generator/commit/c0c1c9b))
- Apply photobook app icon ([03857cd](https://github.com/Ripwords/photobook-generator/commit/03857cd))
- **notifications:** Notify on completed analysis when idle and slow ([f31694b](https://github.com/Ripwords/photobook-generator/commit/f31694b))
- **rust:** Install and initialize tauri-plugin-log ([d56ce84](https://github.com/Ripwords/photobook-generator/commit/d56ce84))
- **sidecar:** Add benchmark NDJSON request for per-stage timing diagnostics ([d951814](https://github.com/Ripwords/photobook-generator/commit/d951814))
- **analysis:** Stream analyze_folder results to the UI via Channel ([e9b7085](https://github.com/Ripwords/photobook-generator/commit/e9b7085))
- **sidecar:** Add smile-proxy calibration diagnostic ([da9643d](https://github.com/Ripwords/photobook-generator/commit/da9643d))
- **templates:** Forbid fold-spanning slots, remove 21 unbuildable templates ([31afe4d](https://github.com/Ripwords/photobook-generator/commit/31afe4d))
- **geometry:** Page-local rects, predicates and spread decomposition ([339bbd2](https://github.com/Ripwords/photobook-generator/commit/339bbd2))
- **templates:** Load spread JSON and decompose into page layouts ([d046234](https://github.com/Ripwords/photobook-generator/commit/d046234))
- **book:** Single culling authority, replacing the Rust/TS duplicate ([e3764ac](https://github.com/Ripwords/photobook-generator/commit/e3764ac))
- **book:** Deterministic saliency- and face-aware crop selection ([0a9fb4d](https://github.com/Ripwords/photobook-generator/commit/0a9fb4d))
- **book:** Template scoring with hard face and gutter constraints ([727ef01](https://github.com/Ripwords/photobook-generator/commit/727ef01))
- **book:** Chapter-aware packing with buildable group sizes ([58b781a](https://github.com/Ripwords/photobook-generator/commit/58b781a))
- **book:** Assemble and pace a full book from scored spreads ([6b2616d](https://github.com/Ripwords/photobook-generator/commit/6b2616d))
- **templates:** Draw the spread library as a visual contact sheet ([ccfbf4a](https://github.com/Ripwords/photobook-generator/commit/ccfbf4a))
- **templates:** Author page-subdivision layouts closing the 4/5/6-up gaps ([77c5005](https://github.com/Ripwords/photobook-generator/commit/77c5005))
- **book:** Pre-flight blocks and warnings before any file is written ([52016f1](https://github.com/Ripwords/photobook-generator/commit/52016f1))
- **project:** Persist a book as a reopenable project ([161cb03](https://github.com/Ripwords/photobook-generator/commit/161cb03))
- **sidecar:** Export cropped source photos as JPEG or PNG ([86f16b4](https://github.com/Ripwords/photobook-generator/commit/86f16b4))
- **export:** Add Rust export client and manifest for the sidecar wire ([d505f41](https://github.com/Ripwords/photobook-generator/commit/d505f41))
- **book:** Generate, save and export a photobook from the UI ([87e8f75](https://github.com/Ripwords/photobook-generator/commit/87e8f75))
- **ui:** Reopen a saved project without re-analysing its folder ([3e934e9](https://github.com/Ripwords/photobook-generator/commit/3e934e9))
- **book:** Let the user include or exclude photos, and honour it ([d923e52](https://github.com/Ripwords/photobook-generator/commit/d923e52))
- **project:** Persist the user's photo overrides with the project ([adc9e02](https://github.com/Ripwords/photobook-generator/commit/adc9e02))
- **commands:** Carry photo overrides across the Rust/webview boundary ([d47a779](https://github.com/Ripwords/photobook-generator/commit/d47a779))
- **ui:** Include and exclude photos from the contact sheet ([9d26dde](https://github.com/Ripwords/photobook-generator/commit/9d26dde))
- Delete and rename saved projects ([706c650](https://github.com/Ripwords/photobook-generator/commit/706c650))
- **preview:** Expose the assembled book's layout to the webview ([d3c9534](https://github.com/Ripwords/photobook-generator/commit/d3c9534))
- **ui:** Render the assembled book, spread by spread, read-only ([c083507](https://github.com/Ripwords/photobook-generator/commit/c083507))
- **pack:** Tag every group with the kind of slot it fills ([84f4725](https://github.com/Ripwords/photobook-generator/commit/84f4725))
- **pack:** Size the first and last slot as page halves, not spreads ([0263adb](https://github.com/Ripwords/photobook-generator/commit/0263adb))
- **score:** Let the seed break ties between middle spreads ([ab41cf9](https://github.com/Ripwords/photobook-generator/commit/ab41cf9))
- **templates:** Add four soft-term weights, all inert ([905ae28](https://github.com/Ripwords/photobook-generator/commit/905ae28))
- **cull:** Carry scene tags and capture time on Photo ([cff8892](https://github.com/Ripwords/photobook-generator/commit/cff8892))
- **score:** Weight Vision's face capture quality ([1628d48](https://github.com/Ripwords/photobook-generator/commit/1628d48))
- **score:** Penalise generic saliency in the gutter ([4d80100](https://github.com/Ripwords/photobook-generator/commit/4d80100))
- **score:** Add a within-spread diversity term ([9b285c4](https://github.com/Ripwords/photobook-generator/commit/9b285c4))
- **score:** Bias template choice toward a dominant hero slot ([d095796](https://github.com/Ripwords/photobook-generator/commit/d095796))
- **scan:** Walk nested folders when analysing, skipping hidden directories and symlinked ones ([847243f](https://github.com/Ripwords/photobook-generator/commit/847243f))
- **book:** Spread-level controls -- regenerate, reject, choose layout, lock, shuffle, swap ([b8fa700](https://github.com/Ripwords/photobook-generator/commit/b8fa700))
- **analysis:** Analyse several folders as one set, and refuse to answer for a set that is not on screen ([a84ba97](https://github.com/Ripwords/photobook-generator/commit/a84ba97))
- **book:** Hand-crop a placement by dragging the photo in its slot and scrolling to zoom ([168d853](https://github.com/Ripwords/photobook-generator/commit/168d853))
- **book:** Move and resize photo boxes with snapping, under the same hard constraints ([a9d8324](https://github.com/Ripwords/photobook-generator/commit/a9d8324))
- **dev:** Answer every UI command in the browser harness ([ea828d8](https://github.com/Ripwords/photobook-generator/commit/ea828d8))
- **ui:** Split the app into a library, a photo picker and a book editor ([ce95a20](https://github.com/Ripwords/photobook-generator/commit/ce95a20))
- **ui:** Give the book a desk, pin the generate bar, and read counts as prose ([1d45b49](https://github.com/Ripwords/photobook-generator/commit/1d45b49))
- **agent:** Build the agent view, the only shape of a book a model sees ([8b19037](https://github.com/Ripwords/photobook-generator/commit/8b19037))
- **agent:** Keep the DeepSeek and Jev keys in the macOS keychain ([c65266f](https://github.com/Ripwords/photobook-generator/commit/c65266f))
- **agent:** Route every model request through one allowlisted Rust command ([bbaa1a0](https://github.com/Ripwords/photobook-generator/commit/bbaa1a0))
- **agent:** Define the agent's tools as one table over agent_view and edit_book ([dcc2f2d](https://github.com/Ripwords/photobook-generator/commit/dcc2f2d))
- **agent:** Stream model requests through Rust with a fetch the AI SDK accepts ([f77f4db](https://github.com/Ripwords/photobook-generator/commit/f77f4db))
- **agent:** Add the Jev client for routing, photo search and proposal checks ([3937170](https://github.com/Ripwords/photobook-generator/commit/3937170))
- **agent:** Run the book agent loop with approval for every write ([13ce4f5](https://github.com/Ripwords/photobook-generator/commit/13ce4f5))
- **agent:** Make Jev optional, so a missing key reads as off ([31c30ee](https://github.com/Ripwords/photobook-generator/commit/31c30ee))
- **chat:** Add the book chat panel and the API keys modal ([3003301](https://github.com/Ripwords/photobook-generator/commit/3003301))
- **ui:** Rebuild the app as a desktop shell with a monochrome theme ([2632617](https://github.com/Ripwords/photobook-generator/commit/2632617))
- **rust:** Let several analyses run and be overridden at once ([350d6be](https://github.com/Ripwords/photobook-generator/commit/350d6be))
- **ui:** Name a book first and keep analysing when you leave ([90dfcb1](https://github.com/Ripwords/photobook-generator/commit/90dfcb1))
- Keep drafts across restarts ([812ff98](https://github.com/Ripwords/photobook-generator/commit/812ff98))
- Undo deleting a book ([0477f4e](https://github.com/Ripwords/photobook-generator/commit/0477f4e))
- Replace a placed photo with any analysed photo ([c2fa6d8](https://github.com/Ripwords/photobook-generator/commit/c2fa6d8))
- Spread a book across moments instead of filling it with bursts ([9a39c33](https://github.com/Ripwords/photobook-generator/commit/9a39c33))
- Right-click a book to star, rename, reveal or delete it ([cc37484](https://github.com/Ripwords/photobook-generator/commit/cc37484))
- **sidecar:** Feature print, clipping fractions and percentile contrast ([86482fe](https://github.com/Ripwords/photobook-generator/commit/86482fe))
- Parse feature prints and clipping, add feature_distance, analyzer v3 ([93798e9](https://github.com/Ripwords/photobook-generator/commit/93798e9))
- **cull:** Group look-alike shots by feature print, not pHash alone ([a4380aa](https://github.com/Ripwords/photobook-generator/commit/a4380aa))
- **cache:** Pure least-recently-used eviction plan that never evicts a pinned photo ([ec3788d](https://github.com/Ripwords/photobook-generator/commit/ec3788d))
- **settings:** Storage section with cache size, limit and Clear unused ([bb917da](https://github.com/Ripwords/photobook-generator/commit/bb917da))
- **cull:** Keep the frame most like its burst, never a clipped one ([f9e2740](https://github.com/Ripwords/photobook-generator/commit/f9e2740))
- **cache:** Enforce the cache limit without breaking saved books ([81d524e](https://github.com/Ripwords/photobook-generator/commit/81d524e))
- **cache:** Enforce the cache limit at startup and after each analysis ([5eaf1fb](https://github.com/Ripwords/photobook-generator/commit/5eaf1fb))
- **release:** Add `bun run release` with checked version stamping ([1c46c1d](https://github.com/Ripwords/photobook-generator/commit/1c46c1d))
- **tests:** Refactor tests to use offCooperativePool for Vision calls to prevent deadlocks ([5141eec](https://github.com/Ripwords/photobook-generator/commit/5141eec))
- **updater:** Install new releases from GitHub ([cbbc4ca](https://github.com/Ripwords/photobook-generator/commit/cbbc4ca))
- **print:** Make print size configurable per book ([b22e44f](https://github.com/Ripwords/photobook-generator/commit/b22e44f))

### 🔥 Performance

- **overrides:** Cache the analysed set in AppState instead of re-uploading it ([ad6e2cf](https://github.com/Ripwords/photobook-generator/commit/ad6e2cf))
- **rust:** Hash photos with the CPU's SHA-256 instructions ([9e30d28](https://github.com/Ripwords/photobook-generator/commit/9e30d28))
- **rust:** Hash a chunk's photos on every core ([342da64](https://github.com/Ripwords/photobook-generator/commit/342da64))
- **sidecar:** Export four photos at once ([8114785](https://github.com/Ripwords/photobook-generator/commit/8114785))
- **ui:** Virtualize the contact sheet ([7a14525](https://github.com/Ripwords/photobook-generator/commit/7a14525))
- **rust:** Skip re-reading files whose size and modified time are unchanged ([e5fd38f](https://github.com/Ripwords/photobook-generator/commit/e5fd38f))

### 🩹 Fixes

- **sidecar:** Preserve request id on decode failure and harden emit against encode errors ([6205658](https://github.com/Ripwords/photobook-generator/commit/6205658))
- **sidecar:** Auto-build sidecar on dev/build, add cross-language ping integration test ([078aec5](https://github.com/Ripwords/photobook-generator/commit/078aec5))
- **sidecar:** Pin EXIF date formatter locale, add GPS/date test coverage ([cc3b44a](https://github.com/Ripwords/photobook-generator/commit/cc3b44a))
- **templates:** Enforce aspect_pref against real-world slot shape ([ee506ae](https://github.com/Ripwords/photobook-generator/commit/ee506ae))
- **sidecar:** Make palette tie-breaks deterministic; align pHash with documented design ([6f71eec](https://github.com/Ripwords/photobook-generator/commit/6f71eec))
- **ui:** Wire up Tailwind and @nuxt/ui stylesheet ([2298e0d](https://github.com/Ripwords/photobook-generator/commit/2298e0d))
- **sidecar:** Make face landmarks image-normalised to match box coordinate space ([fd1bcee](https://github.com/Ripwords/photobook-generator/commit/fd1bcee))
- **sidecar:** Scope smile-proxy landmarks to outer lips only ([f448fef](https://github.com/Ripwords/photobook-generator/commit/f448fef))
- **sidecar:** Widen autoreleasepool to cover thumbnail decode, add PhotoRecord JSON contract tests ([177af08](https://github.com/Ripwords/photobook-generator/commit/177af08))
- **sidecar:** Bound Vision concurrency to prevent analyzer deadlock ([5559112](https://github.com/Ripwords/photobook-generator/commit/5559112))
- **sidecar:** Narrow Vision semaphore guard, add honest watchdog limits ([986c2ae](https://github.com/Ripwords/photobook-generator/commit/986c2ae))
- **sidecar:** Rebuild stress-test watchdog on a raw Thread ([5c44e71](https://github.com/Ripwords/photobook-generator/commit/5c44e71))
- **cluster:** Assign undated group id 0 when no photos have timestamps ([e5a907a](https://github.com/Ripwords/photobook-generator/commit/e5a907a))
- **ranking:** Contain NaN inside percentiles instead of the caller's contract ([f6354d8](https://github.com/Ripwords/photobook-generator/commit/f6354d8))
- **sidecar:** Stop analyze_folder from starving its own drain task ([6b5b283](https://github.com/Ripwords/photobook-generator/commit/6b5b283))
- Round the app icon to Apple's macOS squircle ([283ae4a](https://github.com/Ripwords/photobook-generator/commit/283ae4a))
- Finish PhotobookGen rename and drop debug scratch files ([c24bee9](https://github.com/Ripwords/photobook-generator/commit/c24bee9))
- **rust:** Remove unused fs:default capability permission ([c9066de](https://github.com/Ripwords/photobook-generator/commit/c9066de))
- **rust:** Stale cache paths, silent failures, and AppleDouble files ([8306cde](https://github.com/Ripwords/photobook-generator/commit/8306cde))
- **app:** Stop smileFraction rendering as NaN%, fix event ordering ([211e03e](https://github.com/Ripwords/photobook-generator/commit/211e03e))
- **security:** Harden CSP with base-uri and form-action directives ([a312181](https://github.com/Ripwords/photobook-generator/commit/a312181))
- **sidecar:** Size-aware two-pass decode fixes slow RAW/HEIC analysis ([a9121da](https://github.com/Ripwords/photobook-generator/commit/a9121da))
- **analysis:** Chunk the folder gather so photos stream within ~1s ([4bcde18](https://github.com/Ripwords/photobook-generator/commit/4bcde18))
- **db:** Bump ANALYZER_VERSION for embedded-preview decode change ([7fb5394](https://github.com/Ripwords/photobook-generator/commit/7fb5394))
- **analysis:** Use invoke's return value as the authoritative summary ([d89b3a0](https://github.com/Ripwords/photobook-generator/commit/d89b3a0))
- **scripts:** Make benchmark.sh work on stock macOS bash ([106fd34](https://github.com/Ripwords/photobook-generator/commit/106fd34))
- Forward sidecar stderr diagnostics to log::warn! ([1be9da4](https://github.com/Ripwords/photobook-generator/commit/1be9da4))
- **ui:** Suppress miscalibrated smile indicator, file calibration findings ([ec7be71](https://github.com/Ripwords/photobook-generator/commit/ec7be71))
- **book:** Strengthen the smile-exclusion test, correct a stale doc comment ([9131a49](https://github.com/Ripwords/photobook-generator/commit/9131a49))
- **book:** Never fabricate an unbuildable group size in pack ([fc89c23](https://github.com/Ripwords/photobook-generator/commit/fc89c23))
- **book:** Fall back to a smaller page half instead of blanking the page ([ab3acda](https://github.com/Ripwords/photobook-generator/commit/ab3acda))
- **book:** Adopt Pixajoy's 200 DPI floor and 0.125in safe margin ([fcc7797](https://github.com/Ripwords/photobook-generator/commit/fcc7797))
- **db:** Make foreign-key enforcement an explicit guarantee ([19f97e4](https://github.com/Ripwords/photobook-generator/commit/19f97e4))
- **sidecar:** Report export collisions and out-of-range crops ([0a0502c](https://github.com/Ripwords/photobook-generator/commit/0a0502c))
- **project:** Persist the photo set by hash so a saved book stays exportable ([62faefe](https://github.com/Ripwords/photobook-generator/commit/62faefe))
- **cull:** Make Rust the single authority for the on-screen kept set ([9465e67](https://github.com/Ripwords/photobook-generator/commit/9465e67))
- **pack:** Distribute photos across spread slots instead of front-loading ([36171e6](https://github.com/Ripwords/photobook-generator/commit/36171e6))
- **pack:** Seat every photo the slots can hold, and vary spread density ([eb56f35](https://github.com/Ripwords/photobook-generator/commit/eb56f35))
- **cull:** Refuse a feature record missing a field the book depends on ([7178017](https://github.com/Ripwords/photobook-generator/commit/7178017))
- **ui:** A photo toggle no longer destroys the generated book ([b64e18c](https://github.com/Ripwords/photobook-generator/commit/b64e18c))
- **ui:** Make a reopened project's photo selection visible and editable ([10e4bbf](https://github.com/Ripwords/photobook-generator/commit/10e4bbf))
- **ui:** Default the left-out photos off on a folder too large to render ([6d5ca96](https://github.com/Ripwords/photobook-generator/commit/6d5ca96))
- Repair broken build and close the verification gap that missed it ([b8636b4](https://github.com/Ripwords/photobook-generator/commit/b8636b4))
- **preview:** Draw each page on the side it actually prints on ([7c5c13e](https://github.com/Ripwords/photobook-generator/commit/7c5c13e))
- **pace:** Place groups by the packer's slot tag, not by position ([eff388e](https://github.com/Ripwords/photobook-generator/commit/eff388e))
- **pack:** Merge chapters too small for a spread, and bound the band exactly ([d6ca111](https://github.com/Ripwords/photobook-generator/commit/d6ca111))
- **templates:** Use both page halves in the windowpane and mosaic ([8c28cc6](https://github.com/Ripwords/photobook-generator/commit/8c28cc6))
- **pack:** Seat the chapter remainder a spread slot cannot build ([c0b5966](https://github.com/Ripwords/photobook-generator/commit/c0b5966))
- **commands:** Report the records from_features refused ([eaf5382](https://github.com/Ripwords/photobook-generator/commit/eaf5382))
- **templates:** Remove the confound from the unknown-key weights test ([a572fde](https://github.com/Ripwords/photobook-generator/commit/a572fde))
- **pack:** Size the single pages per side, apportion by slot kind, fold chapters the slots cannot seat ([23d757f](https://github.com/Ripwords/photobook-generator/commit/23d757f))
- **agent:** Give the agent its own edit command so only a rule's refusal reaches the model ([aeaab1c](https://github.com/Ripwords/photobook-generator/commit/aeaab1c))
- **agent:** Ask DeepSeek for deepseek-flash, its name for V4.1 Flash ([5110310](https://github.com/Ripwords/photobook-generator/commit/5110310))
- **agent:** Ask Jev once per proposal and route once per user message ([1209610](https://github.com/Ripwords/photobook-generator/commit/1209610))
- **dev:** Return the analysed photos from the harness's analysis again ([8c0ef18](https://github.com/Ripwords/photobook-generator/commit/8c0ef18))
- **rust:** Send the analysis run id as runId ([2f282bb](https://github.com/Ripwords/photobook-generator/commit/2f282bb))
- **ui:** Quieter swap bar that fits on one line in both themes ([cd1b0e2](https://github.com/Ripwords/photobook-generator/commit/cd1b0e2))
- **cull:** Drop the density term, which complete linkage makes inert ([855d4ca](https://github.com/Ripwords/photobook-generator/commit/855d4ca))

### 💅 Refactors

- **sidecar:** Extract analyze_batches for direct unit testing ([981614e](https://github.com/Ripwords/photobook-generator/commit/981614e))
- **preflight:** Make the pure core actually pure ([79653c4](https://github.com/Ripwords/photobook-generator/commit/79653c4))

### 📖 Documentation

- Add photobook generator design spec ([5dec313](https://github.com/Ripwords/photobook-generator/commit/5dec313))
- Add phase 1 analysis pipeline implementation plan ([e49fa8f](https://github.com/Ripwords/photobook-generator/commit/e49fa8f))
- Apply pre-flight plan corrections ([c677839](https://github.com/Ripwords/photobook-generator/commit/c677839))
- Generate test fixtures with Swift/ImageIO instead of ImageMagick ([8712178](https://github.com/Ripwords/photobook-generator/commit/8712178))
- **templates:** Explain coordinate system, geometric rules, and how to add a template ([32973f0](https://github.com/Ripwords/photobook-generator/commit/32973f0))
- **templates:** Add summary report of the template library ([f85e38a](https://github.com/Ripwords/photobook-generator/commit/f85e38a))
- Mark superseded sections of the phase 1 plan ([8acda4a](https://github.com/Ripwords/photobook-generator/commit/8acda4a))
- Add project status and handoff notes ([9e29fd9](https://github.com/Ripwords/photobook-generator/commit/9e29fd9))
- Record notification state and keepers duplication debt ([b77f838](https://github.com/Ripwords/photobook-generator/commit/b77f838))
- **sidecar:** Correct SmileProxy's outerLips comment, add a warning ([193637f](https://github.com/Ripwords/photobook-generator/commit/193637f))
- **analysis:** Spell out that Batch counts are cumulative, not deltas ([a940832](https://github.com/Ripwords/photobook-generator/commit/a940832))
- **status:** Record ImageLoader double-decode and dead benchmark path ([226a9d0](https://github.com/Ripwords/photobook-generator/commit/226a9d0))
- Commit the engineering reports that shipped code cites ([00bdb7b](https://github.com/Ripwords/photobook-generator/commit/00bdb7b))
- Record multi-folder support as a Phase 2 design question ([25ef954](https://github.com/Ripwords/photobook-generator/commit/25ef954))
- Record confirmed page structure and the single-page template gap ([d2bd9a0](https://github.com/Ripwords/photobook-generator/commit/d2bd9a0))
- Record template coverage gaps for the Phase 2 packer ([005d202](https://github.com/Ripwords/photobook-generator/commit/005d202))
- Spec Phase 2 layout engine and export ([ce05373](https://github.com/Ripwords/photobook-generator/commit/ce05373))
- Add Phase 2 implementation plan ([ba11b04](https://github.com/Ripwords/photobook-generator/commit/ba11b04))
- Revise export design to ship the user's own images ([f0ecc20](https://github.com/Ripwords/photobook-generator/commit/f0ecc20))
- Plan the Phase 2 export revision ([b2e931a](https://github.com/Ripwords/photobook-generator/commit/b2e931a))
- Bring PROJECT-STATUS up to date through Phase 2 ([4dbad00](https://github.com/Ripwords/photobook-generator/commit/4dbad00))
- Correct the Phase 2 docs that describe the abandoned export design ([cf9dc7a](https://github.com/Ripwords/photobook-generator/commit/cf9dc7a))
- Record the two nits from the final fix-wave re-review ([e79eb60](https://github.com/Ripwords/photobook-generator/commit/e79eb60))
- Record user-controlled selection, and two things the docs got wrong ([5a6d978](https://github.com/Ripwords/photobook-generator/commit/5a6d978))
- Record the override round trip's cost, keys and UI limits ([1e706a7](https://github.com/Ripwords/photobook-generator/commit/1e706a7))
- Record the AppState.photos collision in the multi-folder section ([fca0d28](https://github.com/Ripwords/photobook-generator/commit/fca0d28))
- Record the first real-photo run's four quality complaints ([6a40c63](https://github.com/Ripwords/photobook-generator/commit/6a40c63))
- Design the completion of Phase 2 ([fe08f90](https://github.com/Ripwords/photobook-generator/commit/fe08f90))
- Plan the implementation of Phase 2 completion ([9a6fb5b](https://github.com/Ripwords/photobook-generator/commit/9a6fb5b))
- Record what Phase 2 completion closed, and what it did not ([a2a0f88](https://github.com/Ripwords/photobook-generator/commit/a2a0f88))
- **pack:** Record why the side-blind `single` set was NOT narrowed ([26e3af2](https://github.com/Ripwords/photobook-generator/commit/26e3af2))
- **score:** Warn that palette_harmony and palette_distance oppose each other ([8708eeb](https://github.com/Ripwords/photobook-generator/commit/8708eeb))
- Qualify the placement invariant and add open item 14 ([e6f2b58](https://github.com/Ripwords/photobook-generator/commit/e6f2b58))
- Record the 2026-09-16 session -- packer redesign, Phase 3, multi-folder, harness ([7e966c3](https://github.com/Ripwords/photobook-generator/commit/7e966c3))
- List the Phase 4 steps and final counts for 2026-09-16 ([d8d69a1](https://github.com/Ripwords/photobook-generator/commit/d8d69a1))
- Describe the three-screen flow ([a22ac1f](https://github.com/Ripwords/photobook-generator/commit/a22ac1f))
- Plan Phase 5, the agent layer, with Jev as router, ranker and proposal check ([d116519](https://github.com/Ripwords/photobook-generator/commit/d116519))
- Record feature prints, analyzer v3 and the similarity calibration ([8daf6b7](https://github.com/Ripwords/photobook-generator/commit/8daf6b7))
- Record similarity-aware culling, its calibration and new traps ([107c779](https://github.com/Ripwords/photobook-generator/commit/107c779))
- Document the cache limit, its pin set and the eviction trap ([02862f7](https://github.com/Ripwords/photobook-generator/commit/02862f7))
- **readme:** Restructure for the public repo ([9acfb0d](https://github.com/Ripwords/photobook-generator/commit/9acfb0d))

### 📦 Build

- **bundle:** Produce a dmg alongside the .app ([df94841](https://github.com/Ripwords/photobook-generator/commit/df94841))

### 🏡 Chore

- Stop tracking agent worktrees ([37bd094](https://github.com/Ripwords/photobook-generator/commit/37bd094))
- Gitignore .env so API keys cannot be committed ([ddbd1fe](https://github.com/Ripwords/photobook-generator/commit/ddbd1fe))
- Rename app to PhotobookGen ([f0d9757](https://github.com/Ripwords/photobook-generator/commit/f0d9757))
- Remove stray sg.rs ([157919a](https://github.com/Ripwords/photobook-generator/commit/157919a))
- **rust:** Update dependencies for the pre-merge fix wave ([e43a742](https://github.com/Ripwords/photobook-generator/commit/e43a742))
- **rust:** Remove dead Sidecar::ping and Db::hashes_needing_analysis ([cb324cd](https://github.com/Ripwords/photobook-generator/commit/cb324cd))
- **dev:** Browser harness with a stubbed Tauri bridge for looking at the webview ([574df3c](https://github.com/Ripwords/photobook-generator/commit/574df3c))
- Enforce no-explicit-any in oxlint and drop the unused Sidecar::benchmark ([4e69085](https://github.com/Ripwords/photobook-generator/commit/4e69085))
- **bench:** Time the app's analysis path, split by stage ([0ddf894](https://github.com/Ripwords/photobook-generator/commit/0ddf894))
- Merge feat/cache-limit into feat/feature-prints ([bee5cc0](https://github.com/Ripwords/photobook-generator/commit/bee5cc0))
- **github:** Add issue forms and a pull request template ([5996126](https://github.com/Ripwords/photobook-generator/commit/5996126))

### ✅ Tests

- **sidecar:** Replace vacuous blank-background sharpness test ([88444bf](https://github.com/Ripwords/photobook-generator/commit/88444bf))
- **sidecar:** Assert palette composition and pHash Hamming distance ([7227f17](https://github.com/Ripwords/photobook-generator/commit/7227f17))
- **sidecar:** Model realistic burst-frame variation for pHash; document Nyquist limit ([d1c22fd](https://github.com/Ripwords/photobook-generator/commit/d1c22fd))
- **sidecar:** Add hostile input corpus proving crash isolation ([0222eba](https://github.com/Ripwords/photobook-generator/commit/0222eba))
- **db:** Cover version-skew in hashes_needing_analysis; drop unused accessor ([fa2a91a](https://github.com/Ripwords/photobook-generator/commit/fa2a91a))
- **cluster:** Pin the strict gap-threshold boundary in event_clusters ([a8e65dd](https://github.com/Ripwords/photobook-generator/commit/a8e65dd))
- **protocol:** Pin the analyzed response wire format ([e479340](https://github.com/Ripwords/photobook-generator/commit/e479340))
- **sidecar:** Pin cross-language hash agreement and the PhotoFeatures key set ([b4e2656](https://github.com/Ripwords/photobook-generator/commit/b4e2656))
- **geometry:** Property tests on the layout predicates ([61eb086](https://github.com/Ripwords/photobook-generator/commit/61eb086))
- **book:** Pin the DPI floor on a slot that can express it exactly ([4ed8765](https://github.com/Ripwords/photobook-generator/commit/4ed8765))
- **export:** Drive the real sidecar over the export wire ([272270a](https://github.com/Ripwords/photobook-generator/commit/272270a))
- **commands:** Stop the count_keepers fixture lying about its percentiles ([5a0b7bd](https://github.com/Ripwords/photobook-generator/commit/5a0b7bd))
- **book:** Give the packer and pre-flight guards fixtures with teeth ([def66ff](https://github.com/Ripwords/photobook-generator/commit/def66ff))
- **overrides:** Pin the path-vs-hash verdict on content, not on length ([450811f](https://github.com/Ripwords/photobook-generator/commit/450811f))
- **pack:** Cover the merge's last-resort arm, and fix pack.rs's EOF ([429b59e](https://github.com/Ripwords/photobook-generator/commit/429b59e))
- **score:** Isolate each spread_diversity component and pin the hero_prominence double-gate ([6630da8](https://github.com/Ripwords/photobook-generator/commit/6630da8))
- **templates:** Fail the build when a photo slot overlaps a text zone ([2bd4e66](https://github.com/Ripwords/photobook-generator/commit/2bd4e66))
- **sidecar:** Analyse an isolated top-level copy of the fixtures now that the scan recurses ([693a78d](https://github.com/Ripwords/photobook-generator/commit/693a78d))

### 🤖 CI

- Run tests, lint and the Nuxt build check on every push and PR ([0e682ab](https://github.com/Ripwords/photobook-generator/commit/0e682ab))
- **release:** Build and publish the Apple Silicon dmg on v* tags ([c720416](https://github.com/Ripwords/photobook-generator/commit/c720416))
- Time out hung Swift tests and dump their stacks ([af81644](https://github.com/Ripwords/photobook-generator/commit/af81644))

### ❤️ Contributors

- JJ <teohjjteoh@gmail.com>

