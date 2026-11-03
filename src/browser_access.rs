//! Explicit browser origin policy for localhost or an HTTPS reverse proxy.
use axum::http::{HeaderMap, Uri, header};

#[derive(Clone, Default)]
pub struct BrowserAccess {
    public_origin: Option<String>,
    base_path: String,
}
impl BrowserAccess {
    pub fn https(origin: &str) -> Result<Self, &'static str> {
        let value = origin.trim_end_matches('/');
        let uri: Uri = value.parse().map_err(|_| "Invalid public origin")?;
        let authority = uri.authority().ok_or("Public origin needs a hostname")?;
        if uri.scheme_str() != Some("https")
            || authority.as_str().contains('@')
            || uri.host().is_none_or(str::is_empty)
            || value != format!("https://{authority}")
        {
            return Err(
                "SMARTTHERM_PUBLIC_ORIGIN must be an HTTPS origin without credentials, path, query or fragment",
            );
        }
        Ok(Self {
            public_origin: Some(value.to_owned()),
            ..Self::default()
        })
    }
    /// Empty means the root; otherwise a normalized, literal URL prefix.
    pub fn with_base_path(mut self, path: &str) -> Result<Self, &'static str> {
        if !path.is_empty()
            && path != "/"
            && (!path.starts_with('/')
                || path.ends_with('/')
                || path[1..].split('/').any(|part| {
                    part.is_empty()
                        || part == "."
                        || part == ".."
                        || !part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
                }))
        {
            return Err(
                "SMARTTHERM_BASE_PATH must be a literal path like /heating, without a trailing slash",
            );
        }
        self.base_path = if path == "/" {
            String::new()
        } else {
            path.to_owned()
        };
        Ok(self)
    }
    pub fn base_path(&self) -> &str {
        &self.base_path
    }
    pub fn cookie_path(&self) -> String {
        format!("{}/", self.base_path)
    }
    pub fn secure_cookie(&self) -> bool {
        self.public_origin.is_some()
    }

    pub fn allows(&self, headers: &HeaderMap) -> bool {
        let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
            return false;
        };
        let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
            return false;
        };
        let Ok(uri) = origin.parse::<Uri>() else {
            return false;
        };
        if !uri.authority().is_some_and(|a| a.as_str() == host)
            || uri.path() != "/"
            || uri.query().is_some()
        {
            return false;
        }
        match &self.public_origin {
            Some(expected) => origin == expected,
            None => {
                matches!(uri.scheme_str(), Some("http" | "https"))
                    && matches!(uri.host(), Some("127.0.0.1" | "localhost" | "[::1]"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn headers(host: &str, origin: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, host.parse().unwrap());
        headers.insert(header::ORIGIN, origin.parse().unwrap());
        headers
    }
    #[test]
    fn hosted_origin_is_exact_and_never_inferred_from_forwarded_headers() {
        let policy = BrowserAccess::https("https://heating.example.test/").unwrap();
        assert!(policy.secure_cookie());
        assert!(policy.allows(&headers(
            "heating.example.test",
            "https://heating.example.test"
        )));
        for (host, origin) in [
            ("heating.example.test", "http://heating.example.test"),
            ("heating.example.test", "https://evil.example.test"),
            ("heating.example.test", "https://heating.example.test:444"),
            ("localhost:3000", "https://heating.example.test"),
            ("localhost:3000", "http://localhost:3000"),
            ("heating.example.test", "null"),
        ] {
            assert!(!policy.allows(&headers(host, origin)));
        }
        let mut request = headers("evil.example.test", "https://evil.example.test");
        request.insert("x-forwarded-host", "heating.example.test".parse().unwrap());
        request.insert("x-forwarded-proto", "https".parse().unwrap());
        assert!(!policy.allows(&request));
        assert!(!policy.allows(&HeaderMap::new()));
    }
    #[test]
    fn prefix_validation_rejects_ambiguous_or_injectable_paths() {
        for path in ["", "/", "/heating", "/home/heating"] {
            assert!(BrowserAccess::default().with_base_path(path).is_ok());
        }
        for path in [
            "heating",
            "//heating",
            "/heating/",
            "/a/../b",
            "/a//b",
            "/a?b",
            "/a#b",
            "/a%2fb",
            "/a\"b",
            "/a;b",
            "/a\\b",
        ] {
            assert!(
                BrowserAccess::default().with_base_path(path).is_err(),
                "{path}"
            );
        }
    }
    #[test]
    fn local_default_remains_local_and_bad_configuration_fails() {
        let local = BrowserAccess::default();
        assert!(!local.secure_cookie());
        assert!(local.allows(&headers("localhost:5173", "http://localhost:5173")));
        assert!(!local.allows(&headers(
            "heating.example.test",
            "https://heating.example.test"
        )));
        for value in [
            "",
            "http://heating.example.test",
            "https://user@heating.example.test",
            "https://heating.example.test/path",
            "https://heating.example.test?x=1",
            "https://heating.example.test#fragment",
        ] {
            assert!(BrowserAccess::https(value).is_err(), "{value}");
        }
    }
}
