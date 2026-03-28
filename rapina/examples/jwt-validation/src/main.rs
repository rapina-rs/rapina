use rapina::jwt;
use rapina::jwt::{JsonWebToken, JwksClient};
use rapina::prelude::*;

#[derive(Deserialize)]
struct GoogleClaims {
    pub email: String,
}
#[get("/email")]
async fn get_email(token: JsonWebToken<GoogleClaims>) -> Json<String> {
    println!("Token subject: {}", token.sub);
    Json(token.claims.email)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    /*
    To try this with Google's API Playground -- the code given below --, use the following steps:

    1) Navigate to https://developers.google.com/oauthplayground
    2) In "Step 1, Select & authorize APIs" enter: "https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile"
    3) Press "Authorize APIs", proceed with the Google account login
    4) Authorization code grant should be successful, "Step 2" should be uncollapsed now. Press "Exchange authorization code for tokens".
    5) In "Request / Response" section of the website, you will see Google's token output.
       Take the value of field "id_token" and use it as your "Authorization" header to the webserver.
       Typically you will prefix the header value with the static string "Bearer", i.e. "Bearer {your id token here}".
    6) The webserver should respond with the email address after parsing and validating the JWT
     */

    let router = Router::new().get("/email", get_email);

    let discovery_url = "https://accounts.google.com/.well-known/openid-configuration";
    let jwks_client = JwksClient::oidc(discovery_url.to_string());

    /*
    Alternatively use the direct JWKS url to fetch JwksClient::Direct

    let jwks_client = JwksClient::Direct {
        client: jwt::build_http_client(),
        jwks_url: "https://www.googleapis.com/oauth2/v3/certs".to_string(),
    };
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
        .router(router)
        .listen("127.0.0.1:3000")
        .await
}
