use std::{io::Write, path::Path, time::Duration};

use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, Utc};
use oauth2::{
    AccessToken, AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, RedirectUrl,
    RefreshToken, Scope, TokenResponse, TokenUrl, basic::BasicClient, reqwest::redirect::Policy,
    url::Url,
};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncBufReadExt;

macro_rules! parse_param {
    ($url:expr, $key:literal, $type:tt) => {
        $url.query_pairs()
            .find(|(key, _)| key == $key)
            .map(|(_, val)| $type::new(val.into_owned()))
            .ok_or_else(|| anyhow!(concat!("missing ", $key, " parameter")))
    };
}

const DISCORD_AUTHORIZE_URL: &str = "https://discord.com/oauth2/authorize";
const DISCORD_TOKEN_URL: &str = "https://discord.com/api/v10/oauth2/token";

const META_APPLICATION_ID: &str = "1417273808645259344";
const META_REDIRECT_URL: &str = "https://oculus.com/oauth_account_linking/login_redirect";

#[derive(Serialize, Deserialize)]
pub struct TokenInformation {
    pub access_token: AccessToken,
    pub refresh_token: RefreshToken,
    pub expires: DateTime<Utc>,
}

impl TokenInformation {
    pub fn new(
        access_token: AccessToken,
        refresh_token: RefreshToken,
        expires: DateTime<Utc>,
    ) -> Self {
        Self {
            access_token,
            refresh_token,
            expires,
        }
    }

    pub async fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let data = serde_json::to_string(self)?;

        if let Some(p) = path.as_ref().parent() {
            tokio::fs::create_dir_all(p).await?;
        }

        tokio::fs::write(path, data).await?;

        Ok(())
    }

    pub async fn load(path: impl AsRef<Path>) -> Result<Self> {
        let data = tokio::fs::read(path).await?;
        let info = serde_json::from_slice(&data)?;

        Ok(info)
    }
}

pub async fn request_token_interactive() -> Result<TokenInformation> {
    let client = BasicClient::new(ClientId::new(META_APPLICATION_ID.to_string()))
        .set_redirect_uri(
            RedirectUrl::new(META_REDIRECT_URL.to_string()).expect("invalid redirect URL"),
        )
        .set_token_uri(TokenUrl::new(DISCORD_TOKEN_URL.to_string()).expect("invalid token URL"))
        .set_auth_uri(
            AuthUrl::new(DISCORD_AUTHORIZE_URL.to_string()).expect("invalid authorize URL"),
        );
    let (pkce_code_challenge, pkce_code_verifier) = PkceCodeChallenge::new_random_sha256();

    let http_client = oauth2::reqwest::Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("failed to build client");

    let (authorize_url, csrf_state) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("identify".to_string()))
        .add_scope(Scope::new("activities.read".to_string()))
        .add_scope(Scope::new("activities.write".to_string()))
        .set_pkce_challenge(pkce_code_challenge)
        .url();

    print!(
        "Open this URL in your browser:\n{authorize_url}\nAfter performing the authorization, paste the resulting URL here:\n> "
    );

    _ = std::io::stdout().flush();

    let mut stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(&mut stdin);

    let mut line = String::new();
    reader.read_line(&mut line).await?;

    let url = Url::parse(&line)?;

    let code = parse_param!(url, "code", AuthorizationCode)?;
    let state = parse_param!(url, "state", CsrfToken)?;

    // I know I know CSRF is kinda useless here but boohoo let me have some fun
    if csrf_state.secret() != state.secret() {
        bail!("CSRF state mismatch");
    }

    let response = client
        .exchange_code(code)
        .set_pkce_verifier(pkce_code_verifier)
        .request_async(&http_client)
        .await?;

    let expires_in = response.expires_in().unwrap_or(Duration::from_secs(3600));
    let expires = Utc::now() + expires_in;

    Ok(TokenInformation::new(
        response.access_token().clone(),
        response
            .refresh_token()
            .expect("missing refresh token")
            .clone(),
        expires,
    ))
}

pub async fn refresh_token(refresh_token: &RefreshToken) -> Result<TokenInformation> {
    let http_client = oauth2::reqwest::Client::new();
    let client = BasicClient::new(ClientId::new(META_APPLICATION_ID.to_string()))
        .set_token_uri(TokenUrl::new(DISCORD_TOKEN_URL.to_string()).expect("invalid token URL"));

    let response = client
        .exchange_refresh_token(refresh_token)
        .request_async(&http_client)
        .await?;

    let expires_in = response.expires_in().unwrap_or(Duration::from_secs(3600));

    let expires = Utc::now() + expires_in;

    Ok(TokenInformation::new(
        response.access_token().clone(),
        response
            .refresh_token()
            .expect("missing refresh token in response")
            .clone(),
        expires,
    ))
}
