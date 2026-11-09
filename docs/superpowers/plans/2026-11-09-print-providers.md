# Print providers: send a book straight to Pixajoy

**Goal.** One button sends the open book to the user's Pixajoy account. It creates a
project, uploads the photos and lays every page out the way PhotobookGen did. Pixajoy is the
first provider. The module shape must let more providers be added later without touching
the book engine.

**API reference.** `docs/providers/pixajoy.md`. Every call in it was replayed from a script
and checked in Pixajoy's editor on 2026-11-09.

## Hard rules

- Never call `/api/project/order` or anything that adds to a cart.
- Upload happens only when the user presses Send and confirms. The confirmation says that
  the exported photos go to Pixajoy's servers. Analysis stays on-device as before.
- The `mbtk` token lives in the macOS Keychain (the `keyring` pattern in
  `src-tauri/src/agent/keys.rs`). It is never logged, written to disk or put in the repo.
- Never reopen an existing project (`OnlineAPI.editProject` is unreliable). Each send
  creates a new project and fills it in the same editor session.

## Data shape

```rust
// src-tauri/src/providers/mod.rs
pub enum ProviderId { Pixajoy }            // serde "pixajoy"; the registry
pub struct PrintJob {                      // provider-neutral, built purely from Book + export
    pub name: String,
    pub page_count: u32,                   // interior pages, numbered from 1
    pub pages: Vec<JobPage>,
    pub cover: JobCover,                   // front / back: Option<JobImage>
}
pub struct JobPage { pub number: u32, pub boxes: Vec<JobBox>, pub texts: Vec<JobText> }
pub struct JobBox  { pub rect: Rect /* page-normalised */, pub image: JobImage }
pub struct JobText { pub rect: Rect, pub text: String, pub align: Align, pub point_size: f64 }
pub struct JobImage { pub path: PathBuf, pub width: u32, pub height: u32 }  // the exported, already-cropped file
```

The job carries the **exported** files. They are already cropped, upright and RGB, so each
box fill is a plain centred cover fit. Each chapter title with a title spot is a `JobText`
on its page. `plan.rs` clones Pixajoy's unprotected cover text box for it, sets the geometry
and replaces the styled text's `data`. The format is in `docs/providers/pixajoy.md`.

## Modules

1. **Sidecar JPEG output.** Add an output-format option to `ExportRequest` (`auto`, the
   default and the current behaviour, or `jpeg`). Send exports to a temporary folder as JPEG
   (quality ≥ 0.92). Swift tests prefixed `exporter…`.
2. **`providers/job.rs`.** `build_job(book, export_results) -> PrintJob`. Pure. Pixel sizes
   are read from the written files' headers (`imagesize` crate) or returned by the sidecar.
3. **`providers/pixajoy/plan.rs`.** Pure. Takes the parsed `project/load` subset, the job
   and a map from path to asset id. Returns the `project/save` diff as `serde_json::Value`.
   - Uses `layout.width/height` from the load, not constants.
   - Interior page `n` box left is `n × width + rect.x × width`. Top is `rect.y × height`.
   - Deletes every default picture box on every interior page. Then it adds one box per
     job box, cloned from a default box with `ref`/`recordid`/`boxtype` stripped and refs
     -1, -2, …
   - Cover. Finds the picture boxes (`type 0`) on page 100000. The front is the one with
     the largest `boxleft` and the back is the other. Fills them in `changed`. Leaves text
     boxes alone. An empty slot is left untouched.
   - Crop maths per `docs/providers/pixajoy.md`, centred (`ax = ay = 0.5`).
   - `page_count_for(book_pages) -> Result<u32>`. The book must fit 20..=150 pages. Below
     20, pad to 20 with blank pages, and delete the default boxes so they print blank. Odd
     counts round up to even. Above 150 is an error the UI shows before anything uploads.
4. **`providers/pixajoy/client.rs`.** reqwest. Covers create, insert, authenticate, load,
   upload (create → POST → queue → poll), save and reauthenticate on 401. Use a Chrome
   User-Agent on the cart call. Upload with bounded concurrency (4).
5. **`providers/pixajoy/auth.rs`.** "Sign in to Pixajoy" opens a Tauri webview window on
   `https://www.pixajoy.com.my/login`. It polls `cookies_for_url` for `mbtk`, stores it in
   the Keychain and closes the window. Status comes from decoding the JWT payload (no
   signature check), giving the email and expiry. An expired token counts as signed out.
6. **Commands.** `provider_status(provider)`, `provider_sign_in(provider)`,
   `provider_sign_out(provider)`, and `send_to_provider(project_id, provider, on_event: Channel)`.
   Events: `exporting {done,total}`, `creating`, `uploading {done,total}`, `laying_out`,
   `done {project_name, open_url}`, `failed {message, project_name?}`.
7. **UI.** A "Send to Pixajoy" control next to Export in `BookEditor.vue` opens a modal (the
   app's modal, not `window.confirm`). The modal shows:
   - the sign-in state, with Sign in and Sign out
   - what will happen, and that photos are uploaded to Pixajoy
   - the page count, and any padding to 20
   - that chapter titles go onto their pages as editable text
   - progress
   - on success, "Open My Projects" (`https://www.pixajoy.com.my/dashboard/projects`) via the
     opener plugin

   Monochrome. Text at least 14 px.
8. **Docs.** `docs/manual.md` gets the entry and `docs/PROJECT-STATUS.md` a section.

## Verification

- Unit tests on `plan.rs` and `job.rs`:
  - the check case from the API doc (1200 × 800 in an 11.2205 × 8.7402 box gives
    magnification 116.842 and scroll −0.9449)
  - the page-strip offset
  - deleting the defaults
  - negative refs that are unique
  - picking the cover front and back
  - page padding, rounding and the cap
- Mutation-check them in a throwaway worktree, never the live tree.
- A live test `#[ignore]` reads `PIXAJOY_TOKEN` from the environment and sends a small
  fixture book. It names the project `PhotobookGen test <timestamp>`.
- Run one real send and check it in Pixajoy's editor.

## Known gaps, reported and not fixed here

- The product is fixed to IHL8511.
- Titles go as black Arial. The app has no font or colour for them yet.
