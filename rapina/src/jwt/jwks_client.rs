use crate::error::Error;
use bytes::{Buf, Bytes};
use http::Uri;
use http_body_util::{BodyExt, Empty};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use jsonwebtoken::Validation;
use jsonwebtoken::jwk::JwkSet;
use serde::Deserialize;
use serde::de::DeserializeOwned;

pub(crate) type HttpsClient = Client<HttpsConnector<HttpConnector>, Empty<Bytes>>;

#[derive(Clone)]
pub enum JwksClient {
    Direct {
        client: HttpsClient,
        jwks_url: String,
    },
    Oidc {
        client: HttpsClient,
        discovery_url: String,
    },
}

pub(crate) trait JwksProvider {
    fn get_jwks_content(&self) -> impl Future<Output = Result<JwkSet, Error>> + Send;
}

impl JwksClient {
    pub fn oidc(discovery_url: String) -> JwksClient {
        Self::Oidc {
            client: build_http_client(),
            discovery_url,
        }
    }

    pub fn direct(jwks_url: String) -> JwksClient {
        Self::Direct {
            client: build_http_client(),
            jwks_url,
        }
    }
}

impl JwksProvider for JwksClient {
    async fn get_jwks_content(&self) -> Result<JwkSet, Error> {
        match self {
            JwksClient::Direct { client, jwks_url } => {
                fetch_json_content(client, jwks_url).await.map_err(|e| {
                    Error::internal(format!("Failed to retrieve data from JWKS uri: {}", e))
                })
            }
            JwksClient::Oidc {
                client,
                discovery_url,
            } => {
                #[derive(Deserialize)]
                struct OidcConfig {
                    jwks_uri: String,
                }
                let oidc_config: OidcConfig = fetch_json_content(client, discovery_url)
                    .await
                    .map_err(|_| {
                        Error::internal("Failed to retrieve data from OIDC discovery endpoint")
                    })?;

                fetch_json_content(client, &oidc_config.jwks_uri)
                    .await
                    .map_err(|e| {
                        Error::internal(format!("Failed to retrieve data from JWKS uri: {}", e))
                    })
            }
        }
    }
}

pub fn default_validation() -> Validation {
    let mut validation = Validation::default();

    // account for 10 seconds of clock skew per default
    validation.leeway = 10;

    // enable aud (audience), exp (expiration) and nbf (not before) field validation
    validation.validate_aud = true;
    validation.validate_exp = true;
    validation.validate_nbf = true;

    validation
}

pub(crate) fn build_http_client() -> HttpsClient {
    let http_client = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .expect("no native root CA certificates found")
        .https_or_http()
        .enable_http1()
        .build();
    Client::builder(TokioExecutor::new()).build(http_client)
}

async fn fetch_json_content<T: DeserializeOwned>(
    client: &HttpsClient,
    uri: &str,
) -> Result<T, Error> {
    let uri: Uri = uri
        .parse::<Uri>()
        .map_err(|e| Error::internal(format!("Invalid URI: {}", e)))?;

    let res = client
        .get(uri)
        .await
        .map_err(|e| Error::internal(format!("Failed to get data: {}", e)))?;

    let body = res
        .collect()
        .await
        .map_err(|e| Error::internal(format!("Body extractor failed: {}", e)))?
        .aggregate();

    let json: T = serde_json::from_reader(body.reader())
        .map_err(|e| Error::internal(format!("Failed parsing result to JSON: {}", e)))?;
    Ok(json)
}
