### Task 1: Project scaffold

**Files:**
- Create: `package.json`, `nuxt.config.ts`, `.oxlintrc.json`, `vitest.config.ts`, `app/app.vue`, `app/pages/index.vue`
- Create: `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/build.rs`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json`
- Test: `tests/scaffold.test.ts`

**Interfaces:**
- Consumes: nothing
- Produces: a running `bun run tauri dev` window; the `app_lib` Rust crate name used by every later Rust task

- [ ] **Step 1: Write the failing test**

`tests/scaffold.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import config from "../nuxt.config";

describe("nuxt config", () => {
  it("disables SSR because Tauri has no server", () => {
    expect(config.ssr).toBe(false);
  });

  it("does not ignore the app directory", () => {
    expect(config.ignore).toContain("**/src-tauri/**");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `bun run test`
Expected: FAIL — `Cannot find module '../nuxt.config'`

- [ ] **Step 3: Create the scaffold**

`package.json`:

```json
{
  "name": "photobook-generator",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "tauri dev",
    "build": "tauri build",
    "ui:dev": "nuxt dev",
    "generate": "nuxt generate",
    "sidecar": "bash scripts/build-sidecar.sh",
    "test": "vitest run",
    "test:rust": "cargo test --manifest-path src-tauri/Cargo.toml",
    "test:swift": "swift test --package-path sidecar",
    "lint": "oxlint --deny-warnings",
    "lint:fix": "oxlint --fix --deny-warnings",
    "fmt": "oxfmt",
    "postinstall": "nuxt prepare"
  },
  "dependencies": {
    "@nuxt/ui": "^4.10.0",
    "@tauri-apps/api": "^2.11.1",
    "@tauri-apps/plugin-dialog": "^2.4.0",
    "@tauri-apps/plugin-fs": "^2.5.1",
    "nuxt": "^4.5.0",
    "tailwindcss": "^4.3.3"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2.11.4",
    "oxfmt": "^0.60.0",
    "oxlint": "^1.75.0",
    "typescript": "^5.9.0",
    "vitest": "^4.1.10"
  }
}
```

`nuxt.config.ts`:

```ts
export default defineNuxtConfig({
  modules: ["@nuxt/ui"],
  ssr: false,
  devtools: { enabled: true },
  compatibilityDate: "2026-08-12",
  vite: {
    clearScreen: false,
    envPrefix: ["VITE_", "TAURI_"],
    server: { strictPort: true },
  },
  ignore: ["**/src-tauri/**"],
});
```

`vitest.config.ts`:

```ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: { include: ["tests/**/*.test.ts"] },
});
```

`.oxlintrc.json`:

```json
{ "categories": { "correctness": "error", "suspicious": "warn" } }
```

`app/app.vue`:

```vue
<template>
  <UApp>
    <NuxtPage />
  </UApp>
</template>
```

`app/pages/index.vue`:

```vue
<template>
  <main class="p-8">
    <h1 class="text-2xl font-semibold">Photobook Generator</h1>
  </main>
</template>
```

`src-tauri/Cargo.toml`:

```toml
[package]
name = "photobook-generator"
version = "0.1.0"
edition = "2021"
rust-version = "1.77.2"

[lib]
name = "app_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[[bin]]
name = "PhotobookGenerator"
path = "src/main.rs"

[build-dependencies]
tauri-build = { version = "2.6.3", features = [] }

[dependencies]
tauri = { version = "2.11.3", features = [] }
tauri-plugin-shell = "2"
tauri-plugin-dialog = "2"
tauri-plugin-fs = "2"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
rusqlite = { version = "0.32", features = ["bundled"] }
sha2 = "0.10"
thiserror = "2"
log = "0.4"
```

`src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

`src-tauri/src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    app_lib::run()
}
```

`src-tauri/src/lib.rs`:

```rust
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`src-tauri/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Photobook Generator",
  "version": "0.1.0",
  "identifier": "com.jiajingteoh.photobook",
  "build": {
    "beforeDevCommand": "bun run ui:dev",
    "devUrl": "http://localhost:3000",
    "beforeBuildCommand": "bun run generate",
    "frontendDist": "../.output/public"
  },
  "app": {
    "windows": [{ "title": "Photobook Generator", "width": 1400, "height": 900 }],
    "security": { "csp": null }
  },
  "bundle": {
    "active": true,
    "targets": "app",
    "macOS": { "minimumSystemVersion": "15.0" }
  }
}
```

`src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "windows": ["main"],
  "permissions": ["core:default", "dialog:default", "fs:default"]
}
```

Note `frontendDist` is `../.output/public`, **not** `../dist` — Nuxt 4 generates there and the root `dist` symlink can confuse the macOS bundler.

- [ ] **Step 4: Run tests to verify they pass**

Run: `bun install && bun run test`
Expected: PASS, 2 tests

Then confirm the app launches: `bun run dev`. Expected: a window titled "Photobook Generator".

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: scaffold Tauri 2 + Nuxt 4 project"
```

---

