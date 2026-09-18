# Phase 2: keys in the keychain

[Overview](overview.md)

## Goal

The user can store a DeepSeek key and a Jev key. Rust can read them, and the webview can only
learn whether each is set.

## Changes

- `keyring` 4 in `Cargo.toml`.
- New `src-tauri/src/agent/keys.rs`. A `Provider` enum (`DeepSeek`, `Jev`) owns its keychain
  entry name. It has `set`, `clear` and `status` operations behind a small `KeyStore` trait,
  so tests use an in-memory store and never touch the real keychain.
- Commands `set_api_key(provider, key)`, `clear_api_key(provider)` and `api_key_status()`
  returning `{ deepseek: bool, jev: bool }`. No command ever returns a key.
- Debug builds fall back to `DEEPSEEK_API_KEY` / `TYPESAFE_API_KEY` from the environment when
  the keychain has nothing. Release builds do not read the environment.

## Data structures

- `enum Provider { DeepSeek, Jev }`, serialised as `"deepseek"` / `"jev"`.
- `trait KeyStore { get, set, clear }` with `KeychainStore` and `MemoryStore`.

## Verification

- Set, status, clear round-trip on `MemoryStore`.
- An empty or whitespace key is refused.
- No command's return type can carry the key. A compile-level check: `api_key_status`
  returns a struct of bools only.
- Manual: set a key in `bun run dev`, confirm it appears in Keychain Access under the
  app's service name, and clear it.
