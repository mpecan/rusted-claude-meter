use meter_core::SessionKey;
use reqwest::header::{
    ACCEPT, COOKIE, HeaderMap, HeaderValue, InvalidHeaderValue, ORIGIN, REFERER, USER_AGENT,
};

/// The endpoint sits behind Cloudflare and rejects obviously non-browser
/// clients, so requests present themselves as Chrome — the same approach the
/// Swift app uses.
const CHROME_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// Request headers for claude.ai API calls, including the session cookie.
pub fn browser_headers(session_key: &SessionKey) -> Result<HeaderMap, InvalidHeaderValue> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(CHROME_USER_AGENT));
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(REFERER, HeaderValue::from_static("https://claude.ai/"));
    headers.insert(ORIGIN, HeaderValue::from_static("https://claude.ai"));
    headers.insert("Sec-Fetch-Dest", HeaderValue::from_static("empty"));
    headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("cors"));
    headers.insert("Sec-Fetch-Site", HeaderValue::from_static("same-origin"));

    let cookie = format!("sessionKey={}", session_key.expose());
    let mut cookie = HeaderValue::from_str(&cookie)?;
    cookie.set_sensitive(true);
    headers.insert(COOKIE, cookie);
    Ok(headers)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn cookie_header_is_marked_sensitive() {
        let key = SessionKey::parse("sk-ant-sid01-abcDEF123456_-xyz789").unwrap();
        let headers = browser_headers(&key).unwrap();
        assert!(headers.get(COOKIE).unwrap().is_sensitive());
    }

    #[test]
    fn presents_as_a_browser() {
        let key = SessionKey::parse("sk-ant-sid01-abcDEF123456_-xyz789").unwrap();
        let headers = browser_headers(&key).unwrap();
        assert!(
            headers
                .get(USER_AGENT)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("Chrome")
        );
        assert_eq!(headers.get(ORIGIN).unwrap(), "https://claude.ai");
    }
}
