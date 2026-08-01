use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;
use tracing::error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("resource already exists")]
    Conflict,
    #[error("access denied")]
    Forbidden,
    #[error("internal server error")]
    Internal,
    #[error("identity provider is unavailable")]
    IdentityProviderUnavailable,
    #[error("resource not found")]
    NotFound,
    #[error("service instance is not ready")]
    ServiceInstanceNotReady,
    #[error("too many requests")]
    TooManyRequests,
    #[error("authentication required")]
    Unauthorized,
}

impl ApiError {
    pub fn from_store(error: heterocloud_store::StoreError) -> Self {
        match error {
            heterocloud_store::StoreError::AlreadyExists => Self::Conflict,
            heterocloud_store::StoreError::InvitationUnavailable => {
                Self::BadRequest("The invitation is invalid or no longer available.".into())
            }
            heterocloud_store::StoreError::NotFound => Self::NotFound,
            heterocloud_store::StoreError::Sql(ref sql_error) if is_unique_violation(sql_error) => {
                Self::Conflict
            }
            other => {
                error!(error = %other, "storage operation failed");
                Self::Internal
            }
        }
    }
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code == "23505")
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "bad_request", message.as_str()),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "conflict",
                "A resource with the same identifier already exists.",
            ),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "The authenticated principal is not authorized.",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                "The request could not be completed.",
            ),
            Self::IdentityProviderUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "identity_provider_unavailable",
                "The configured identity provider is temporarily unavailable.",
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "not_found",
                "The requested resource was not found.",
            ),
            Self::ServiceInstanceNotReady => (
                StatusCode::CONFLICT,
                "service_instance_not_ready",
                "The Flow service instance is not ready for access contexts.",
            ),
            Self::TooManyRequests => (
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_requests",
                "Too many expensive authentication requests are in progress.",
            ),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "Authentication is required.",
            ),
        };
        (
            status,
            Json(ErrorEnvelope {
                error: ErrorBody { code, message },
            }),
        )
            .into_response()
    }
}

#[derive(Serialize)]
struct ErrorEnvelope<'a> {
    error: ErrorBody<'a>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}
