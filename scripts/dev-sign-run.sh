#!/bin/sh
# Cargo runner (src-tauri/.cargo/config.toml): signs the dev app before it runs.
#
# `cargo run` links an ad-hoc signed binary whose identity is a hash of the
# build, so the keychain treats every rebuild as a new app and asks for the
# login password again, and "Always Allow" never sticks. Signing with the
# machine's Apple Development certificate under the bundle id gives every
# rebuild the same identity, so one "Always Allow" lasts.
#
# Only the app binary is signed; test binaries run as they are. With no
# Apple Development certificate on the machine it signs nothing and the
# old prompt-per-build behaviour returns.
bin="$1"
if [ "$(uname)" = Darwin ] && [ "$(basename "$bin")" = PhotobookGen ]; then
  identity=$(security find-identity -v -p codesigning 2>/dev/null | awk '/"Apple Development/ { print $2; exit }')
  if [ -n "$identity" ]; then
    codesign --force --sign "$identity" --identifier com.jiajingteoh.photobook "$bin" 2>/dev/null \
      || echo "dev-sign-run: codesign failed; the keychain will ask again" >&2
  fi
fi
exec "$@"
