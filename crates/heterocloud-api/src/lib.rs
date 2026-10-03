pub mod config;
pub mod error;
pub mod flash_provider;
pub mod flow_access;
pub mod metrics;
pub mod oidc;
mod quota_usage;
pub mod registry;
pub mod routes;
pub mod secret_manager;
pub mod syouyu_provider;
pub mod vpc_provider;

use std::{path::Path, sync::Arc};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderName, HeaderValue, header},
};
use routes::AppState;
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::{SetSensitiveRequestHeadersLayer, SetSensitiveResponseHeadersLayer},
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
    trace::TraceLayer,
};

pub fn app(state: Arc<AppState>, console_dir: Option<&Path>) -> Router {
    let request_id = HeaderName::from_static("x-request-id");
    let router = Router::new().nest("/api/v1", routes::api_router(state));
    let router = match console_dir {
        Some(directory) => router.fallback_service(console_files(directory)),
        None => router,
    };
    router
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(
                "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
                 img-src 'self' data:; font-src 'self' data:; connect-src 'self'; \
                 base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
            ),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            HeaderName::from_static("permissions-policy"),
            HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
        ))
        .layer(SetSensitiveResponseHeadersLayer::new([header::SET_COOKIE]))
        .layer(SetSensitiveRequestHeadersLayer::new([
            header::AUTHORIZATION,
            header::COOKIE,
        ]))
        .layer(CatchPanicLayer::new())
        .layer(CompressionLayer::new())
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
        .layer(TraceLayer::new_for_http())
}

fn console_files(directory: &Path) -> Router {
    // Public HTML is served directly; console deep links retain their SPA.
    // Keep compatibility with console artifacts built before the public site.
    let console = directory.join("console.html");
    let entry = if console.is_file() {
        console
    } else {
        directory.join("index.html")
    };
    // HTML at / used to be the console. Require revalidation so browsers do
    // not retain that entry (or an obsolete JS entry) across deployments.
    // Conditional requests can still reuse unchanged files with HTTP 304.
    Router::new()
        .fallback_service(ServeDir::new(directory).fallback(ServeFile::new(entry)))
        .layer(SetResponseHeaderLayer::overriding(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-cache"),
        ))
}

#[cfg(test)]
mod public_site_tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use std::{fs, path::PathBuf};
    use tower::ServiceExt;

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    struct SiteDirectory(PathBuf);

    impl SiteDirectory {
        fn new() -> std::io::Result<Self> {
            let path =
                std::env::temp_dir().join(format!("heterocloud-site-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(path.join("technology"))?;
            Ok(Self(path))
        }
    }

    impl Drop for SiteDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    async fn get(router: Router, path: &str) -> TestResult<String> {
        let response = router
            .oneshot(Request::builder().uri(path).body(Body::empty())?)
            .await?;
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        let body = axum::body::to_bytes(response.into_body(), 4096).await?;
        Ok(String::from_utf8(body.to_vec())?)
    }

    #[tokio::test]
    async fn public_html_and_console_deep_links_use_distinct_entries() -> TestResult {
        let directory = SiteDirectory::new()?;
        fs::write(directory.0.join("index.html"), "public introduction")?;
        fs::write(
            directory.0.join("technology/index.html"),
            "technology article",
        )?;
        fs::write(directory.0.join("console.html"), "console application")?;
        let router = Router::new().fallback_service(console_files(&directory.0));
        assert_eq!(get(router.clone(), "/").await?, "public introduction");
        assert_eq!(
            get(router.clone(), "/technology/").await?,
            "technology article"
        );
        for path in [
            "/login",
            "/console",
            "/overview",
            "/flash/services/example",
            "/cli/authorize?user_code=TEST-CODE",
        ] {
            assert_eq!(get(router.clone(), path).await?, "console application");
        }
        Ok(())
    }

    #[tokio::test]
    async fn older_console_artifacts_keep_their_spa_fallback() -> TestResult {
        let directory = SiteDirectory::new()?;
        fs::write(directory.0.join("index.html"), "older console")?;
        let router = Router::new().fallback_service(console_files(&directory.0));
        assert_eq!(
            get(router, "/cli/authorize?user_code=TEST-CODE").await?,
            "older console"
        );
        Ok(())
    }

    #[tokio::test]
    async fn conditional_and_head_requests_keep_revalidation_headers() -> TestResult {
        let directory = SiteDirectory::new()?;
        fs::write(directory.0.join("index.html"), "public introduction")?;
        fs::write(directory.0.join("console.html"), "console application")?;
        let router = Router::new().fallback_service(console_files(&directory.0));
        for path in ["/", "/console"] {
            let head = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("HEAD")
                        .uri(path)
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(head.status(), axum::http::StatusCode::OK);
            assert_eq!(head.headers()[header::CACHE_CONTROL], "no-cache");
            let modified = head.headers()[header::LAST_MODIFIED].clone();
            let cached = router
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header(header::IF_MODIFIED_SINCE, modified)
                        .body(Body::empty())?,
                )
                .await?;
            assert_eq!(cached.status(), axum::http::StatusCode::NOT_MODIFIED);
            assert_eq!(cached.headers()[header::CACHE_CONTROL], "no-cache");
        }
        Ok(())
    }
}
