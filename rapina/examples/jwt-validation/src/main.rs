mod authz;

use rapina::jwt;
use rapina::jwt::{JsonWebToken, JwksClient};
use rapina::prelude::*;

#[derive(Deserialize, Clone)]
struct GoogleClaims {
    pub email: String,
}

// Example authorization handler that compares the token subject field (.sub) to a hardcoded string
async fn authorization_handler(token: &JsonWebToken<GoogleClaims>) -> Result<()> {
    tracing::info!(sub = %token.sub, "authorizing request before it hits the handler");
    if "{YOUR GOOGLE USER ID HERE TO PASS THE AUTHORIZATION LOGIC}" == token.sub.as_str() {
        return Ok(());
    }
    Err(Error::forbidden("Missing permissions"))
}

// Example handler that takes two parameters and is authorized by an authorization handler within another module
#[get("/email")]
#[authorize(authz::authorize(JsonWebToken))]
async fn get_email(token: JsonWebToken<GoogleClaims>, _unused: Headers) -> Result<Json<String>> {
    tracing::info!(sub = %token.sub, "authenticated request");
    Ok(Json(token.claims.email))
}

// Example handler that takes two parameters and is authorized by an authorization handler within the same module
#[get("/example1")]
#[authorize(authorization_handler(JsonWebToken<GoogleClaims>))]
async fn ping(_unused: JsonWebToken) -> Result<Json<String>> {
    tracing::info!("this is called within the handler body");
    Ok(Json("success".to_string()))
}

// Example handler that takes no parameter and is authorized by an authorization handler within another module
#[get("/example2")]
#[authorize(authz::authorize(JsonWebToken))]
async fn pong() -> Result<Json<String>> {
    tracing::info!("this is called within the handler body");
    Ok(Json("success".to_string()))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    /*
    To try this with Google's API Playground -- the code given below --, use the following steps:

    1) Navigate to https://developers.google.com/oauthplayground
    2) In "Step 1, Select & authorize APIs" copy the following string into the text field with label "Input your own scopes": "https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile"
    3) Press "Authorize APIs", proceed with the Google account login
    4) Authorization code grant should be successful, "Step 2" should be uncollapsed now. Press "Exchange authorization code for tokens".
    5) In "Request / Response" section of the website, you will see Google's token output.
       Take the value of field "id_token" and use it as your "Authorization" header to the webserver.
       Typically you will prefix the header value with the static string "Bearer", i.e. "Bearer {your id token here}".
    6) The webserver should respond with the email address after parsing and validating the JWT
     */
    tracing_subscriber::fmt().init();

    // OIDC Discovery endpoint of Google Accounts API
    let discovery_url = "https://accounts.google.com/.well-known/openid-configuration";

    // Cron schedule of 5 minutes to periodically refresh the JWKS content
    let cron_refresh_schedule = "0 */5 * * * *";

    let jwks_client =
        JwksClient::oidc(discovery_url.to_string(), cron_refresh_schedule.to_string());

    /*
    Alternatively use the direct JWKS url to fetch the content (using JwksClient::Direct)

    let jwks_client = JwksClient::direct(
        "https://www.googleapis.com/oauth2/v3/certs".to_string(),
        cron_refresh_schedule.to_string(),
    );
    */

    // Enable the audience validation (this is a _must have_ in production environments!).
    // Only turn it off deliberately by calling "jwks_validation.validate_aud = false" if you know what you are doing!
    const GOOGLE_OAUTH_PLAYGROUND_AUDIENCE: &str = "407408718192.apps.googleusercontent.com";
    const GOOGLE_ISSUER: &str = "https://accounts.google.com";

    let mut jwks_validation = jwt::default_validation();
    jwks_validation.set_audience(&[GOOGLE_OAUTH_PLAYGROUND_AUDIENCE]);
    jwks_validation.set_issuer(&[GOOGLE_ISSUER]);

    Rapina::new()
        .state(jwks_client)
        .state(jwks_validation)
        .discover()
        .listen("127.0.0.1:3000")
        .await
}
