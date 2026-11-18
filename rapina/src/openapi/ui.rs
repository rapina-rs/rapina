//! Interactive documentation served from assets embedded in the binary.
use crate::response::full;
use crate::router::Router;
use http::{
    Response, StatusCode,
    header::{CACHE_CONTROL, CONTENT_TYPE, ETAG, IF_NONE_MATCH},
};
use std::hash::{DefaultHasher, Hasher};

/// Selects the interactive documentation served at `/docs`.
#[derive(Debug, Clone, Copy)]
pub enum OpenApiUi {
    /// Scalar API Reference.
    #[cfg(feature = "openapi-scalar")]
    Scalar,
    /// Swagger UI.
    #[cfg(feature = "openapi-swagger")]
    Swagger,
}

struct Asset {
    path: &'static str,
    mime: &'static str,
    content: &'static str,
}

impl Asset {
    const fn new(path: &'static str, mime: &'static str, content: &'static str) -> Self {
        Self {
            path,
            mime,
            content,
        }
    }
}

impl OpenApiUi {
    fn assets(self) -> &'static [Asset] {
        match self {
            #[cfg(feature = "openapi-scalar")]
            Self::Scalar => {
                const ASSETS: &[Asset] = &[
                    Asset::new(
                        "/docs",
                        "text/html; charset=utf-8",
                        include_str!("assets/scalar.html"),
                    ),
                    Asset::new(
                        "/__rapina/docs/ui.js",
                        "text/javascript",
                        include_str!("assets/scalar.js"),
                    ),
                    Asset::new(
                        "/__rapina/docs/init.js",
                        "text/javascript",
                        include_str!("assets/scalar-init.js"),
                    ),
                    Asset::new(
                        "/__rapina/docs/LICENSE.txt",
                        "text/plain",
                        include_str!("assets/SCALAR-LICENSE"),
                    ),
                ];
                ASSETS
            }
            #[cfg(feature = "openapi-swagger")]
            Self::Swagger => {
                const ASSETS: &[Asset] = &[
                    Asset::new(
                        "/docs",
                        "text/html; charset=utf-8",
                        include_str!("assets/swagger.html"),
                    ),
                    Asset::new(
                        "/__rapina/docs/ui.js",
                        "text/javascript",
                        include_str!("assets/swagger-ui-bundle.js"),
                    ),
                    Asset::new(
                        "/__rapina/docs/init.js",
                        "text/javascript",
                        include_str!("assets/swagger-init.js"),
                    ),
                    Asset::new(
                        "/__rapina/docs/LICENSE.txt",
                        "text/plain",
                        concat!(
                            include_str!("assets/LICENSE"),
                            "\n",
                            include_str!("assets/NOTICE")
                        ),
                    ),
                    Asset::new(
                        "/__rapina/docs/ui.css",
                        "text/css",
                        include_str!("assets/swagger-ui.css"),
                    ),
                    Asset::new(
                        "/__rapina/docs/swagger-ui-bundle.js.LICENSE.txt",
                        "text/plain",
                        include_str!("assets/swagger-ui-bundle.js.LICENSE.txt"),
                    ),
                ];
                ASSETS
            }
        }
    }

    pub(crate) fn conflicting_path(self, router: &Router) -> Option<&'static str> {
        self.assets()
            .iter()
            .find(|asset| {
                router.routes.iter().any(|(method, route)| {
                    method == http::Method::GET && route.pattern == asset.path
                })
            })
            .map(|asset| asset.path)
    }

    pub(crate) fn mount(self, mut router: Router) -> Router {
        for asset in self.assets() {
            let mut hash = DefaultHasher::new();
            hash.write(asset.content.as_bytes());
            let tag = format!("\"{:016x}\"", hash.finish());
            // A weak validator also applies when middleware compresses the response.
            let etag = http::HeaderValue::from_str(&format!("W/{tag}")).unwrap();
            router = router.get_named(asset.path, "openapi_ui", move |req, _, _| {
                let unchanged = req
                    .headers()
                    .get_all(IF_NONE_MATCH)
                    .iter()
                    .filter_map(|value| value.to_str().ok())
                    .flat_map(|value| value.split(','))
                    .map(str::trim)
                    .any(|value| value == "*" || value.strip_prefix("W/").unwrap_or(value) == tag);
                let response = Response::builder()
                    .header(ETAG, etag.clone())
                    .header(CACHE_CONTROL, "no-cache");
                async move {
                    if unchanged {
                        response
                            .status(StatusCode::NOT_MODIFIED)
                            .body(full(""))
                            .unwrap()
                    } else {
                        response
                            .header(CONTENT_TYPE, asset.mime)
                            .body(full(asset.content))
                            .unwrap()
                    }
                }
            });
        }
        router
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::Rapina, testing::TestClient};

    #[tokio::test]
    async fn openapi_ui_serves_selected_assets() {
        for (ui, swagger) in [
            #[cfg(feature = "openapi-scalar")]
            (OpenApiUi::Scalar, false),
            #[cfg(feature = "openapi-swagger")]
            (OpenApiUi::Swagger, true),
        ] {
            let router = Router::new().get_named("/docs", "custom_docs", |_, _, _| async {
                "application docs"
            });
            let existing = TestClient::new(
                Rapina::new()
                    .router(router)
                    .openapi("Test", "1")
                    .openapi_ui(ui),
            )
            .await;
            assert_eq!(
                existing.get("/docs").send().await.text(),
                "application docs"
            );
            assert_eq!(
                existing.get("/__rapina/docs/ui.js").send().await.status(),
                404
            );

            let client = TestClient::new(Rapina::new().openapi_ui(ui).openapi("Test", "1")).await;
            for path in ["/docs", "/__rapina/docs/ui.js", "/__rapina/docs/init.js"] {
                assert_eq!(client.get(path).send().await.status(), 200);
            }
            let script = client.get("/__rapina/docs/ui.js").send().await;
            let tag = script.headers()[ETAG].to_str().unwrap();
            assert_eq!(script.headers()[CACHE_CONTROL], "no-cache");
            let cached = client
                .get("/__rapina/docs/ui.js")
                .header("if-none-match", tag)
                .send()
                .await;
            assert_eq!(cached.status(), 304);
            assert!(cached.text().is_empty());
            let stale = client
                .get("/__rapina/docs/ui.js")
                .header("if-none-match", "\"old\"")
                .send()
                .await;
            assert_eq!(stale.status(), 200);

            for asset in ui.assets() {
                let router =
                    Router::new().get_named(asset.path, "existing", |_, _, _| async { "original" });
                let app = TestClient::new(
                    Rapina::new()
                        .router(router)
                        .openapi("Test", "1")
                        .openapi_ui(ui),
                )
                .await;
                assert_eq!(app.get(asset.path).send().await.text(), "original");
                if asset.path != "/docs" {
                    assert_eq!(app.get("/docs").send().await.status(), 404);
                }
            }
            let page = client.get("/docs").send().await.text();
            assert!(!page.contains("<script>"));
            assert!(page.contains(r#"<script src="./__rapina/docs/init.js"></script>"#));
            assert!(!page.contains(r#"="/__rapina/"#));
            let license = client.get("/__rapina/docs/LICENSE.txt").send().await.text();
            if swagger {
                assert!(license.contains("Apache License"));
                assert!(license.contains("SmartBear"));
            } else {
                assert!(license.contains("MIT License"));
                assert!(license.contains("Scalar"));
            }
            let notice = client
                .get("/__rapina/docs/swagger-ui-bundle.js.LICENSE.txt")
                .send()
                .await;
            assert_eq!(notice.status(), if swagger { 200 } else { 404 });
            let css = client.get("/__rapina/docs/ui.css").send().await;
            assert_eq!(css.status(), if swagger { 200 } else { 404 });
        }
    }
}
