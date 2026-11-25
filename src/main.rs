use std::{ops::ControlFlow, time::Duration};

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::oauth::TokenInformation;

mod oauth;

#[derive(Serialize, Deserialize)]
struct HeadlessSession {
    activities: Vec<Activity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Activity {
    application_id: String,
    name: String,
    r#type: u32,
    platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let auth_file = std::env::var("AUTH_FILE_PATH").unwrap_or_else(|_| "auth.json".to_string());

    let mut auth = match TokenInformation::load(&auth_file).await {
        Ok(cached) => cached,
        Err(_) => {
            let auth = oauth::request_token_interactive().await?;
            auth.save(&auth_file).await?;

            auth
        }
    };

    let activity = Activity {
        name: "~~".into(),
        application_id: "1417273808645259344".into(), // Doesn't look like this matters as long as the app exists
        r#type: 6, // The HANG type does not replace your custom status
        platform: "meta_quest".into(),
        session_id: None,
    };

    let client = reqwest::Client::new();
    let mut token = None;

    loop {
        if auth.expires < Utc::now() + Duration::from_secs(5 * 60) {
            auth = oauth::refresh_token(&auth.refresh_token).await?;
            auth.save(&auth_file).await?;
        }

        token = match update_status(
            &client,
            auth.access_token.secret(),
            token.as_ref(),
            &activity,
        )
        .await
        {
            Ok(token) => token,
            Err(why) => {
                eprintln!("An error has occured while updating your status: {why}");
                break;
            }
        };

        if wait().await.is_break() {
            break;
        }
    }

    if let Some(token) = token {
        _ = delete_status(&client, auth.access_token.secret(), &token).await;
    }

    Ok(())
}

async fn update_status(
    client: &reqwest::Client,
    access_token: &str,
    session_token: Option<&String>,
    activity: &Activity,
) -> Result<Option<String>> {
    let response = client
        .post("https://discord.com/api/v10/users/@me/headless-sessions")
        .header("Content-Type", "application/json")
        .bearer_auth(access_token)
        .json(&HeadlessSession {
            activities: vec![activity.clone()],
            token: session_token.cloned(),
        })
        .send()
        .await?;

    let session = response
        .error_for_status()?
        .json::<HeadlessSession>()
        .await?;

    Ok(session.token)
}

async fn delete_status(
    client: &reqwest::Client,
    access_token: &str,
    session_token: &str,
) -> Result<()> {
    client
        .post("https://discord.com/api/v10/users/@me/headless-sessions/delete")
        .header("Content-Type", "application/json")
        .bearer_auth(access_token)
        .json(&json!({
            "token": session_token,
        }))
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}

async fn wait() -> ControlFlow<()> {
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(60)) => {
            ControlFlow::Continue(())
        }

        _ = tokio::signal::ctrl_c() => {
            ControlFlow::Break(())
        }
    }
}
