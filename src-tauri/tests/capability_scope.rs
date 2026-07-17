//! Regression check for the opener plugin's URL scope (issue #10 review).
//!
//! The Safari "Open Settings" button deep-links to the macOS Full Disk Access
//! pane via a custom `x-apple.systempreferences:` URL. The opener plugin's
//! `opener:default` permission set only allows `mailto:`, `tel:`, `http://`
//! and `https://` URLs, so without an explicit scope grant the plugin rejects
//! the deep link with `ForbiddenUrl` — and the button silently does nothing.
//! That failure mode is invisible to every runtime test (it only surfaces by
//! clicking the button in a packaged app), so this test pins the capability
//! file itself: the configured scope must cover the deep-link URL.

#![allow(clippy::unwrap_used)]

use meter_core::FULL_DISK_ACCESS_SETTINGS_URL;

/// The capability granted to the main window, as checked into the repo.
const CAPABILITY: &str = include_str!("../capabilities/default.json");

/// Does an opener-scope glob `pattern` cover `url`? The plugin matches with
/// `glob::Pattern`; the subset used here (literal text plus `*` wildcards) is
/// enough to verify coverage without pulling in the glob crate.
fn glob_covers(pattern: &str, url: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == url,
        Some((prefix, rest)) => url.strip_prefix(prefix).is_some_and(|remaining| {
            (0..=remaining.len())
                .filter(|&skip| remaining.is_char_boundary(skip))
                .any(|skip| glob_covers(rest, &remaining[skip..]))
        }),
    }
}

#[test]
fn opener_scope_covers_the_full_disk_access_deep_link() {
    let capability: serde_json::Value = serde_json::from_str(CAPABILITY).unwrap();
    let permissions = capability["permissions"].as_array().unwrap();

    let allowed_urls: Vec<&str> = permissions
        .iter()
        .filter(|permission| permission["identifier"] == "opener:allow-open-url")
        .filter_map(|permission| permission["allow"].as_array())
        .flatten()
        .filter_map(|entry| entry["url"].as_str())
        .collect();

    assert!(
        allowed_urls
            .iter()
            .any(|pattern| glob_covers(pattern, FULL_DISK_ACCESS_SETTINGS_URL)),
        "capabilities/default.json must grant opener:allow-open-url a scope covering \
         {FULL_DISK_ACCESS_SETTINGS_URL:?}; found only {allowed_urls:?} — without it the \
         Safari \"Open Settings\" button is silently blocked by the opener plugin's ACL"
    );
}

#[test]
fn the_glob_helper_matches_like_the_plugin_scope() {
    assert!(glob_covers(
        "x-apple.systempreferences:*",
        FULL_DISK_ACCESS_SETTINGS_URL
    ));
    assert!(glob_covers("https://*", "https://claude.ai/settings"));
    assert!(!glob_covers("https://*", "mailto:someone@example.com"));
    assert!(!glob_covers(
        "x-apple.systempreferences:",
        FULL_DISK_ACCESS_SETTINGS_URL
    ));
}
