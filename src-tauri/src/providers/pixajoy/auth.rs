//! Signing in to Pixajoy. The user signs in on Pixajoy's own login page in a
//! window of the app's; the `mbtk` cookie it sets is copied into the macOS
//! Keychain and the window closes. The token is read only by Rust and never
//! logged or written anywhere else.

use crate::agent::keys::KEYCHAIN_SERVICE;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

const ACCOUNT: &str = "pixajoy";
/// Pixajoy has no login page: `/login` renders its 404. Signed out, My Projects
/// shows the login form and, once signed in, the projects themselves.
const LOGIN_URL: &str = "https://www.pixajoy.com.my/dashboard/projects";
const COOKIE_URL: &str = "https://www.pixajoy.com.my/";
const COOKIE: &str = "mbtk";
const WINDOW: &str = "pixajoy-login";
/// Google, Apple and Facebook sign in through a `window.open` popup that posts
/// back to its opener, so popups get windows of their own.
const POPUP_PREFIX: &str = "pixajoy-login-popup-";

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub signed_in: bool,
    pub email: Option<String>,
}

#[derive(Deserialize)]
struct Claims {
    emailaddress: Option<String>,
    exp: Option<i64>,
}

/// Reads the JWT payload without checking its signature: Pixajoy checks it,
/// and the app only needs the email and the expiry to show. An unreadable or
/// expired token counts as signed out.
pub fn status_of(token: Option<&str>, now: i64) -> Status {
    let signed_out = Status { signed_in: false, email: None };
    let Some(payload) = token.and_then(|t| t.split('.').nth(1)) else { return signed_out };
    let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload.trim_end_matches('='))
    else {
        return signed_out;
    };
    let Ok(claims) = serde_json::from_slice::<Claims>(&bytes) else { return signed_out };
    match claims.exp {
        Some(exp) if exp > now => Status { signed_in: true, email: claims.emailaddress },
        _ => signed_out,
    }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, ACCOUNT).map_err(|e| e.to_string())
}

fn stored() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn status() -> Result<Status, String> {
    Ok(status_of(stored()?.as_deref(), now()))
}

/// The stored token, when it is still valid.
pub fn token() -> Result<String, String> {
    let token = stored()?.filter(|t| status_of(Some(t), now()).signed_in);
    token.ok_or_else(|| "Sign in to Pixajoy first.".to_string())
}

pub fn sign_out() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Opens Pixajoy's login page and waits until it sets a valid `mbtk` cookie,
/// or until the user closes the window. The window is private (no cookies
/// kept between sign-ins), so signing out and in again can switch accounts,
/// and it gets no access to the app's commands: capabilities name `main`
/// only.
pub async fn sign_in(app: &AppHandle) -> Result<Status, String> {
    close_login_windows(app);
    let login = LOGIN_URL.parse().map_err(|e| format!("{e}"))?;
    let cookie_url: tauri::Url = COOKIE_URL.parse().map_err(|e| format!("{e}"))?;
    let window = WebviewWindowBuilder::new(app, WINDOW, WebviewUrl::External(login))
        .title("Sign in to Pixajoy")
        .inner_size(520.0, 760.0)
        .incognito(true)
        .on_new_window(popup_handler(app.clone()))
        .build()
        .map_err(|e| e.to_string())?;

    loop {
        tokio::time::sleep(Duration::from_millis(750)).await;
        if app.get_webview_window(WINDOW).is_none() {
            close_login_windows(app);
            return status();
        }
        let Ok(cookies) = window.cookies_for_url(cookie_url.clone()) else { continue };
        let Some(token) = cookies.iter().find(|c| c.name() == COOKIE).map(|c| c.value().to_string())
        else {
            continue;
        };
        let status = status_of(Some(&token), now());
        if status.signed_in {
            entry()?.set_password(&token).map_err(|e| e.to_string())?;
            close_login_windows(app);
            return Ok(status);
        }
    }
}

/// Opens a popup in a window that shares the login window's WebKit
/// configuration, so it sees the same private cookies and keeps
/// `window.opener`. WebKit's own `window.close()` does not reach Tauri, so
/// popups are closed with the login window instead.
fn popup_handler<R: Runtime>(
    app: AppHandle<R>,
) -> impl Fn(tauri::Url, tauri::webview::NewWindowFeatures) -> NewWindowResponse<R> + Send + 'static {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    move |url, features| {
        let label = format!("{POPUP_PREFIX}{}", NEXT.fetch_add(1, Ordering::Relaxed));
        let built = WebviewWindowBuilder::new(&app, label, WebviewUrl::External(url))
            .title("Sign in to Pixajoy")
            .inner_size(520.0, 680.0)
            .window_features(features)
            .build();
        match built {
            Ok(window) => NewWindowResponse::Create { window },
            Err(err) => {
                log::warn!("could not open a sign-in popup: {err}");
                NewWindowResponse::Deny
            }
        }
    }
}

fn close_login_windows<R: Runtime>(app: &AppHandle<R>) {
    for (label, window) in app.webview_windows() {
        if is_login_window(&label) {
            let _ = window.close();
        }
    }
}

fn is_login_window(label: &str) -> bool {
    label == WINDOW || label.starts_with(POPUP_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jwt(payload: &str) -> String {
        let b = |s: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s);
        format!("{}.{}.{}", b(r#"{"alg":"RS256"}"#), b(payload), b("sig"))
    }

    #[test]
    fn popups_close_with_the_login_window_and_nothing_else_does() {
        assert!(is_login_window("pixajoy-login"));
        assert!(is_login_window("pixajoy-login-popup-3"));
        assert!(!is_login_window("main"));
    }

    #[test]
    fn a_current_token_is_signed_in_with_its_email() {
        let t = jwt(r#"{"emailaddress":"a@b.c","userid":"1","exp":2000}"#);
        assert_eq!(status_of(Some(&t), 1999), Status { signed_in: true, email: Some("a@b.c".into()) });
    }

    #[test]
    fn an_expired_token_is_signed_out() {
        let t = jwt(r#"{"emailaddress":"a@b.c","exp":2000}"#);
        assert_eq!(status_of(Some(&t), 2000), Status { signed_in: false, email: None });
    }

    #[test]
    fn no_token_or_garbage_is_signed_out() {
        assert!(!status_of(None, 0).signed_in);
        assert!(!status_of(Some("not-a-jwt"), 0).signed_in);
        assert!(!status_of(Some(&jwt(r#"{"emailaddress":"a@b.c"}"#)), 0).signed_in, "no exp");
    }
}
