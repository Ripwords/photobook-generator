# Wire fixtures: the Rust <-> TypeScript boundary, pinned from both sides

There is no shared schema across this boundary. `AnalysisSummary.photos` is
`Vec<serde_json::Value>` on the Rust side, and every command's return type is
hand-mirrored by an interface in `app/types/`. A field renamed on one side and
not the other is a silent `undefined` at runtime, not a build error --
`docs/PROJECT-STATUS.md` names this as a known debt.

Every JSON file in this directory is that missing schema, in the only form
both languages can read: **a literal committed fixture that each side asserts
against independently.**

- **Rust** (`src-tauri/src/commands.rs`, the `wire fixtures` test section)
  builds the real struct, `serde_json::to_value`s it, and asserts it equals
  the fixture *exactly*. Renaming a Rust field, dropping a `rename_all`, or
  adding a field fails there.
- **TypeScript** (`tests/book.test.ts`) parses the same file and feeds it
  through the actual functions `app/components/GenerateBook.vue` calls, then
  asserts real derived values. Reading a key the fixture does not have yields
  `undefined`, and those assertions fail.

The pairing is what makes this work. A Rust-to-Rust round-trip cannot catch a
rename, because both sides of it move together; neither can a TypeScript test
over a literal it wrote itself. But a rename on either side breaks one of the
two suites against a fixture that did *not* move, and "fixing" the fixture to
match then breaks the other. This mirrors how `protocol.rs` pins the Swift
wire format, and how `hash_matches_the_literal_pinned_against_swifts_content_hash`
pins one literal hash string from both Rust and Swift.

**If you change a wire type, change the fixture in the same commit -- and
expect the other language's suite to tell you what else has to move.**

## Two exceptions worth knowing about

- **`book-layout.json`'s Rust half lives in `src-tauri/src/preview.rs`**, not
  in `commands.rs`, so it can reuse the `Book` fixture that module's other
  tests already build. Its TypeScript half is `tests/preview.test.ts`. The
  contract is otherwise identical.
  **`agent-view.json`** follows the same pattern: its Rust half is in
  `src-tauri/src/agent/view.rs` and its TypeScript half is
  `tests/agent-view.test.ts`.
  **`model-events.json`** pins `model_request`'s channel events and its
  rejection errors. Its Rust half is in `src-tauri/src/agent/request.rs`, which
  builds the refused error by calling `run` with a bad path and the missing-key
  and keychain errors through `From<KeyError>`, so their real messages are
  pinned too. Its TypeScript half is `tests/model-fetch.test.ts`, which replays
  the events and errors through `modelFetch` in `app/agent/fetch.ts`.
  **`spec-check.json`** pins `check_print_spec`'s answer: one `checked` and
  three `refused`, one per `SpecError` shape the Print size panel words. Its
  Rust half is in `src-tauri/src/book/reprint.rs`, which builds each case from
  a real `RawPrintSpec` through `check`. Its TypeScript half is
  `tests/printSpec.test.ts`, which feeds every case through `refusalText`,
  `refusalField` and `checkSummary`. The checked case uses a 16 x 8 page with
  quarter-inch insets so every guide rect is an exact binary fraction and
  `to_value` compares directly, unlike `book-layout.json`.
- **`book-layout.json` is the only fixture carrying an irrational float**
  (`PreviewGeometry`'s guide rects, which are `0.197 / 11.197` and friends
  derived from the book's `PrintSpec`). serde_json's *default* float parser is the fast approximate
  one -- the `float_roundtrip` feature is not enabled -- so reading the fixture
  back lands up to one ULP from the constant that wrote it. That test
  therefore serialises to TEXT and parses BOTH sides with the same parser,
  comparing the JSON text rather than two f64s. Every other fixture holds
  integers and short decimals and compares `to_value` directly; do not copy
  the text round-trip into those.

  **To regenerate it**, serialise with `serde_json::to_value(&layout)` and
  pretty-print that `Value` -- it sorts the keys and keeps the exact f64s.
  Never pretty-print text that has been parsed back: that moves the last digit
  of every irrational inset, and the diff then shows every guide as changed
  when none did. Do it from a scratch edit, not a switch left in the test; a
  test that can write the fixture it asserts against passes by construction.
