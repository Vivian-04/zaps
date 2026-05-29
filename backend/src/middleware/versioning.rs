use axum::{
    extract::Request,
    http::{header::HeaderName, HeaderMap, HeaderValue},
    middleware::Next,
    response::Response,
};
use crate::service::MetricsService;

/// Supported API versions
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ApiVersion {
    V1,
    V2,
}

impl ApiVersion {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApiVersion::V1 => "v1",
            ApiVersion::V2 => "v2",
        }
    }

    pub fn as_numeric(&self) -> u32 {
        match self {
            ApiVersion::V1 => 1,
            ApiVersion::V2 => 2,
        }
    }

    /// Whether this version is deprecated
    pub fn is_deprecated(&self) -> bool {
        matches!(self, ApiVersion::V1)
    }

    /// Sunset date for deprecated versions (RFC 2822 HTTP-date format)
    pub fn sunset_date(&self) -> Option<&'static str> {
        match self {
            ApiVersion::V1 => Some("Sun, 01 Jan 2027 00:00:00 GMT"),
            ApiVersion::V2 => None,
        }
    }

    /// Migration guide URL for deprecated versions
    pub fn migration_guide_url(&self) -> Option<&'static str> {
        match self {
            ApiVersion::V1 => Some("https://docs.blinks.app/api/migration/v1-to-v2"),
            ApiVersion::V2 => None,
        }
    }

    /// Parse version from path segment (e.g. "v1", "v2")
    pub fn from_path_segment(segment: &str) -> Option<Self> {
        match segment {
            "v1" => Some(ApiVersion::V1),
            "v2" => Some(ApiVersion::V2),
            _ => None,
        }
    }

    /// Parse from a header or query-string value — accepts "v1", "v2", "1", "2"
    pub fn from_header_value(value: &str) -> Option<Self> {
        match value.trim() {
            "v1" | "1" => Some(ApiVersion::V1),
            "v2" | "2" => Some(ApiVersion::V2),
            _ => None,
        }
    }

    /// The latest stable API version
    pub fn latest() -> Self {
        ApiVersion::V2
    }
}

/// Stored in request extensions so downstream handlers can inspect the active version.
#[derive(Debug, Clone, Copy)]
pub struct ApiVersionExtension(pub ApiVersion);

/// Axum middleware that resolves the API version, stores it in request extensions,
/// and injects version/deprecation headers into responses.
///
/// Version resolution order:
///   1. Path segment (`/api/v2/...`)
///   2. Request header (`X-API-Version`, `Accept-Version`, or `API-Version`)
///   3. Default to V1 for backward compatibility
///
/// Response headers added:
///   - `X-API-Version`   — resolved version string
///   - `Deprecation`     — "true" when the version is deprecated (RFC 8594)
///   - `Sunset`          — removal date for deprecated versions
///   - `Link`            — URL to the migration guide
///   - `Warning`         — RFC 7234 299 warning with human-readable deprecation notice
pub async fn version_middleware(mut request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();

    let version = extract_version_from_path(&path)
        .or_else(|| extract_version_from_headers(request.headers()))
        .unwrap_or(ApiVersion::V1);

    // Make the resolved version available to handlers
    request.extensions_mut().insert(ApiVersionExtension(version));

    MetricsService::record_api_version_usage(version.as_str(), &path);

    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    if let Ok(val) = HeaderValue::from_str(version.as_str()) {
        headers.insert(HeaderName::from_static("x-api-version"), val);
    }

    if version.is_deprecated() {
        headers.insert(
            HeaderName::from_static("deprecation"),
            HeaderValue::from_static("true"),
        );

        if let Some(sunset) = version.sunset_date() {
            if let Ok(val) = HeaderValue::from_str(sunset) {
                headers.insert(HeaderName::from_static("sunset"), val);
            }
        }

        if let Some(guide_url) = version.migration_guide_url() {
            let link_value = format!("<{}>; rel=\"deprecation\"", guide_url);
            if let Ok(val) = HeaderValue::from_str(&link_value) {
                headers.insert(HeaderName::from_static("link"), val);
            }
        }

        // RFC 7234 §5.5 Warning header — code 299 = "Miscellaneous persistent warning"
        let warning = format!(
            r#"299 - "API {} is deprecated and will be removed on {}. Migrate to {}: {}""#,
            version.as_str(),
            version.sunset_date().unwrap_or("TBD"),
            ApiVersion::latest().as_str(),
            version.migration_guide_url().unwrap_or("https://docs.blinks.app/api"),
        );
        if let Ok(val) = HeaderValue::from_str(&warning) {
            headers.insert(HeaderName::from_static("warning"), val);
        }

        tracing::warn!(
            api_version = version.as_str(),
            path = %path,
            sunset = version.sunset_date().unwrap_or("unknown"),
            "Deprecated API version used"
        );
    }

    response
}

fn extract_version_from_path(path: &str) -> Option<ApiVersion> {
    for segment in path.split('/') {
        if let Some(version) = ApiVersion::from_path_segment(segment) {
            return Some(version);
        }
    }
    None
}

fn extract_version_from_headers(headers: &HeaderMap) -> Option<ApiVersion> {
    for header_name in &["x-api-version", "accept-version", "api-version"] {
        if let Some(val) = headers.get(*header_name) {
            if let Ok(s) = val.to_str() {
                if let Some(v) = ApiVersion::from_header_value(s) {
                    return Some(v);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_version_from_path() {
        assert_eq!(extract_version_from_path("/api/v1/payments"), Some(ApiVersion::V1));
        assert_eq!(extract_version_from_path("/api/v2/payments"), Some(ApiVersion::V2));
        assert_eq!(extract_version_from_path("/health"), None);
    }

    #[test]
    fn test_v1_is_deprecated() {
        assert!(ApiVersion::V1.is_deprecated());
        assert!(!ApiVersion::V2.is_deprecated());
    }

    #[test]
    fn test_sunset_date() {
        assert!(ApiVersion::V1.sunset_date().is_some());
        assert!(ApiVersion::V2.sunset_date().is_none());
    }

    #[test]
    fn test_from_header_value() {
        assert_eq!(ApiVersion::from_header_value("v1"), Some(ApiVersion::V1));
        assert_eq!(ApiVersion::from_header_value("v2"), Some(ApiVersion::V2));
        assert_eq!(ApiVersion::from_header_value("1"), Some(ApiVersion::V1));
        assert_eq!(ApiVersion::from_header_value("2"), Some(ApiVersion::V2));
        assert_eq!(ApiVersion::from_header_value("  v2  "), Some(ApiVersion::V2));
        assert_eq!(ApiVersion::from_header_value("v3"), None);
        assert_eq!(ApiVersion::from_header_value(""), None);
    }

    #[test]
    fn test_latest_is_v2() {
        assert_eq!(ApiVersion::latest(), ApiVersion::V2);
    }

    #[test]
    fn test_as_numeric() {
        assert_eq!(ApiVersion::V1.as_numeric(), 1);
        assert_eq!(ApiVersion::V2.as_numeric(), 2);
    }

    #[test]
    fn test_version_ordering() {
        assert!(ApiVersion::V1 < ApiVersion::V2);
    }
}
