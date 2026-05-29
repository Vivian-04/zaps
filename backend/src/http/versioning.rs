use axum::{
    extract::{Path, Query},
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{api_error::ApiError, middleware::versioning::ApiVersion};

// ---------------------------------------------------------------------------
// Request / query types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct NegotiateQuery {
    /// The version the client would like to use (e.g. "v1", "v2", "1", "2")
    pub requested: Option<String>,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct NegotiationResponse {
    pub recommended_version: String,
    pub requested_version: Option<String>,
    pub is_deprecated: bool,
    pub sunset_date: Option<String>,
    pub migration_guide_url: Option<String>,
    pub available_versions: Vec<String>,
    pub latest_version: String,
}

#[derive(Debug, Serialize)]
pub struct DeprecationNotice {
    pub version: String,
    pub deprecated_since: &'static str,
    pub sunset_date: &'static str,
    pub migration_guide_url: &'static str,
    pub reason: &'static str,
    pub affected_endpoints: Vec<&'static str>,
    pub replacement_version: String,
}

#[derive(Debug, Serialize)]
pub struct DeprecationListResponse {
    pub deprecations: Vec<DeprecationNotice>,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct EndpointInfo {
    pub method: &'static str,
    pub path: &'static str,
    pub description: &'static str,
    pub added_in: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated_in: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct VersionCapabilities {
    pub version: String,
    pub status: &'static str,
    pub endpoints: Vec<EndpointInfo>,
    pub features: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct CompatibilityInfo {
    pub version: String,
    pub is_supported: bool,
    pub is_deprecated: bool,
    pub status: &'static str,
    pub sunset_date: Option<&'static str>,
    /// Number of days until the version is removed; negative means already past sunset.
    pub days_until_sunset: Option<i64>,
    pub recommended_version: String,
    pub upgrade_required_by: Option<&'static str>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/versioning/negotiate
///
/// Given an optional `requested` query parameter, returns the recommended API
/// version alongside deprecation details so clients can make an informed choice.
///
/// Example: `GET /api/versioning/negotiate?requested=v1`
pub async fn negotiate_version(
    Query(query): Query<NegotiateQuery>,
) -> Json<NegotiationResponse> {
    let available = vec!["v1".to_string(), "v2".to_string()];
    let latest = ApiVersion::latest();

    let (requested_str, is_deprecated, sunset_date, migration_guide_url) =
        match query.requested.as_deref() {
            Some(req) => match ApiVersion::from_header_value(req) {
                Some(version) => (
                    Some(version.as_str().to_string()),
                    version.is_deprecated(),
                    version.sunset_date().map(String::from),
                    version.migration_guide_url().map(String::from),
                ),
                None => (Some(req.to_string()), false, None, None),
            },
            None => (None, false, None, None),
        };

    Json(NegotiationResponse {
        recommended_version: latest.as_str().to_string(),
        requested_version: requested_str,
        is_deprecated,
        sunset_date,
        migration_guide_url,
        available_versions: available,
        latest_version: latest.as_str().to_string(),
    })
}

/// GET /api/versioning/deprecations
///
/// Returns all active deprecation notices so clients can proactively plan
/// migrations without waiting for sunset.
pub async fn list_deprecations() -> Json<DeprecationListResponse> {
    let deprecations = build_deprecation_notices();
    let total = deprecations.len();
    Json(DeprecationListResponse { deprecations, total })
}

/// GET /api/versioning/:version/capabilities
///
/// Returns the full list of endpoints and feature flags available in the
/// requested API version.  Useful for feature detection and documentation.
pub async fn get_version_capabilities(
    Path(version): Path<String>,
) -> Result<Json<VersionCapabilities>, ApiError> {
    match version.as_str() {
        "v1" => Ok(Json(v1_capabilities())),
        "v2" => Ok(Json(v2_capabilities())),
        _ => Err(ApiError::NotFound(format!(
            "API version '{}' not found. Supported versions: v1, v2",
            version
        ))),
    }
}

/// GET /api/versioning/compatibility/:version
///
/// Returns lifecycle status for the given version, including how many days
/// remain before sunset, so clients can build their own upgrade-urgency logic.
pub async fn check_compatibility(
    Path(version): Path<String>,
) -> Result<Json<CompatibilityInfo>, ApiError> {
    let api_version =
        ApiVersion::from_header_value(&version).ok_or_else(|| {
            ApiError::NotFound(format!(
                "API version '{}' not found. Supported versions: v1, v2",
                version
            ))
        })?;

    let days_until_sunset = api_version
        .sunset_date()
        .and_then(|s| chrono::DateTime::parse_from_rfc2822(s).ok())
        .map(|sunset| {
            let sunset_utc = sunset.with_timezone(&Utc);
            sunset_utc.signed_duration_since(Utc::now()).num_days()
        });

    let (status, is_deprecated) = if api_version.is_deprecated() {
        ("deprecated", true)
    } else {
        ("current", false)
    };

    Ok(Json(CompatibilityInfo {
        version: api_version.as_str().to_string(),
        is_supported: true,
        is_deprecated,
        status,
        sunset_date: api_version.sunset_date(),
        days_until_sunset,
        recommended_version: ApiVersion::latest().as_str().to_string(),
        upgrade_required_by: api_version.sunset_date(),
    }))
}

// ---------------------------------------------------------------------------
// Data helpers
// ---------------------------------------------------------------------------

fn build_deprecation_notices() -> Vec<DeprecationNotice> {
    vec![DeprecationNotice {
        version: "v1".to_string(),
        deprecated_since: "2026-05-01",
        sunset_date: "2027-01-01",
        migration_guide_url: "https://docs.blinks.app/api/migration/v1-to-v2",
        reason: "v2 introduces payment dispute management, enhanced analytics, and improved \
                 response schemas. v1 will no longer receive feature updates.",
        affected_endpoints: vec![
            "POST /api/v1/payments",
            "GET  /api/v1/payments/:id",
            "GET  /api/v1/payments/:id/status",
            "POST /api/v1/transfers",
            "POST /api/v1/withdrawals",
        ],
        replacement_version: "v2".to_string(),
    }]
}

fn v1_capabilities() -> VersionCapabilities {
    VersionCapabilities {
        version: "v1".to_string(),
        status: "deprecated",
        features: vec![
            "payments",
            "payouts",
            "transfers",
            "withdrawals",
            "identity",
            "notifications",
            "profiles",
            "files",
            "batches",
            "audit-logs",
            "analytics",
            "reconciliation",
            "webhooks",
        ],
        endpoints: vec![
            EndpointInfo {
                method: "POST",
                path: "/api/v1/payments",
                description: "Create a payment",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v1/payments/:id",
                description: "Get payment by ID",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v1/payments/:id/status",
                description: "Get payment status",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "POST",
                path: "/api/v1/transfers",
                description: "Create a transfer",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v1/transfers/:id",
                description: "Get transfer by ID",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "POST",
                path: "/api/v1/withdrawals",
                description: "Create a withdrawal",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "POST",
                path: "/api/v1/payouts",
                description: "Create a payout",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "POST",
                path: "/api/v1/identity/users",
                description: "Create a user identity",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v1/identity/users/me",
                description: "Get current user",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v1/notifications",
                description: "List notifications",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v1/audit-logs",
                description: "List audit logs (admin)",
                added_in: "v1",
                deprecated_in: None,
            },
        ],
    }
}

fn v2_capabilities() -> VersionCapabilities {
    VersionCapabilities {
        version: "v2".to_string(),
        status: "current",
        features: vec![
            "payments",
            "payment-disputes",
            "payouts",
            "transfers",
            "withdrawals",
            "identity",
            "notifications",
            "profiles",
            "files",
            "batches",
            "audit-logs",
            "analytics",
            "reconciliation",
            "webhooks",
            "enhanced-caching",
            "version-analytics",
        ],
        endpoints: vec![
            EndpointInfo {
                method: "POST",
                path: "/api/v2/payments",
                description: "Create a payment (response includes dispute_eligible field)",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v2/payments/:id",
                description: "Get payment by ID (response includes active_dispute field)",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v2/payments/:id/status",
                description: "Get payment status",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "POST",
                path: "/api/v2/payments/:payment_id/disputes",
                description: "File a dispute for a payment",
                added_in: "v2",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v2/payments/:payment_id/disputes",
                description: "List disputes for a payment",
                added_in: "v2",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v2/disputes",
                description: "List all disputes (admin)",
                added_in: "v2",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v2/disputes/me",
                description: "List disputes for current user",
                added_in: "v2",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v2/disputes/:id",
                description: "Get dispute by ID",
                added_in: "v2",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "PATCH",
                path: "/api/v2/disputes/:id/status",
                description: "Update dispute status",
                added_in: "v2",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "POST",
                path: "/api/v2/disputes/:id/evidence",
                description: "Add evidence to a dispute",
                added_in: "v2",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "POST",
                path: "/api/v2/transfers",
                description: "Create a transfer",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "POST",
                path: "/api/v2/withdrawals",
                description: "Create a withdrawal",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "POST",
                path: "/api/v2/payouts",
                description: "Create a payout",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "POST",
                path: "/api/v2/identity/users",
                description: "Create a user identity",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v2/notifications",
                description: "List notifications",
                added_in: "v1",
                deprecated_in: None,
            },
            EndpointInfo {
                method: "GET",
                path: "/api/v2/audit-logs",
                description: "List audit logs (admin)",
                added_in: "v1",
                deprecated_in: None,
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v1_capabilities_status_is_deprecated() {
        let caps = v1_capabilities();
        assert_eq!(caps.status, "deprecated");
    }

    #[test]
    fn test_v2_capabilities_includes_disputes() {
        let caps = v2_capabilities();
        assert!(caps.features.contains(&"payment-disputes"));
        assert!(caps
            .endpoints
            .iter()
            .any(|e| e.path.contains("disputes")));
    }

    #[test]
    fn test_v2_dispute_endpoints_added_in_v2() {
        let caps = v2_capabilities();
        for ep in caps.endpoints.iter().filter(|e| e.path.contains("disputes")) {
            assert_eq!(ep.added_in, "v2");
        }
    }

    #[test]
    fn test_deprecation_notices_cover_v1() {
        let notices = build_deprecation_notices();
        assert!(!notices.is_empty());
        assert_eq!(notices[0].version, "v1");
        assert_eq!(notices[0].replacement_version, "v2");
    }

    #[test]
    fn test_unknown_version_returns_none() {
        assert!(ApiVersion::from_header_value("v99").is_none());
    }
}
