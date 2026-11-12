//! The mood dial: anyone at the party nudges the auto mix from chill to
//! hype (`/api/mood`), and the admin switches it on or off and sets how
//! much it counts (`/api/party/mood`, Admin > Smart play).

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::{AdminUser, CurrentUser};
use crate::error::{ApiError, ApiResult};
use crate::jukebox::{MoodError, MoodState, DEFAULT_MOOD_STRENGTH, MOOD_DIAL, MOOD_STRENGTH};
use crate::settings;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/mood", get(mood).put(nudge))
        .route("/party/mood", put(set_settings))
}

/// The dial in your room.
async fn mood(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
) -> ApiResult<Json<MoodState>> {
    let jukebox = state.jukebox_of(user.id).await;
    let mood = jukebox.lock().await.mood_state();
    Ok(Json(mood))
}

#[derive(Deserialize)]
struct Nudge {
    /// -1 chill to 1 hype.
    value: f32,
}

/// Your setting for your room's dial.
async fn nudge(
    CurrentUser(user): CurrentUser,
    State(state): State<AppState>,
    Json(body): Json<Nudge>,
) -> ApiResult<Json<MoodState>> {
    if !body.value.is_finite() || !(-1.0..=1.0).contains(&body.value) {
        return Err(ApiError::bad_request("the mood is -1 (chill) to 1 (hype)"));
    }
    let jukebox = state.jukebox_of(user.id).await;
    let mood = jukebox.lock().await.vote_mood(user.id, body.value).await;
    match mood {
        Ok(mood) => Ok(Json(mood)),
        Err(MoodError::Off) => Err(ApiError::conflict("the mood dial is off")),
        Err(MoodError::Other(e)) => Err(e.into()),
    }
}

/// The admin's switches for the dial.
#[derive(Debug, Serialize, Deserialize)]
pub struct MoodSettings {
    pub on: bool,
    /// How much it moves the odds, 0 to 100.
    pub strength: i64,
}

pub async fn mood_settings(state: &AppState) -> anyhow::Result<MoodSettings> {
    Ok(MoodSettings {
        on: settings::flag(&state.store, MOOD_DIAL, true).await?,
        strength: settings::number(&state.store, MOOD_STRENGTH, DEFAULT_MOOD_STRENGTH).await?,
    })
}

async fn set_settings(
    _: AdminUser,
    State(state): State<AppState>,
    Json(body): Json<MoodSettings>,
) -> ApiResult<StatusCode> {
    if !(0..=100).contains(&body.strength) {
        return Err(ApiError::bad_request("strength is 0 to 100"));
    }
    settings::set_flag(&state.store, MOOD_DIAL, body.on).await?;
    state
        .store
        .set_setting(MOOD_STRENGTH, &body.strength.to_string())
        .await?;
    for jukebox in state.all_jukeboxes() {
        jukebox.lock().await.set_mood_on(body.on).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::http::Method;
    use djinn_store::Role;

    use super::*;
    use crate::test_util::{call, get_body, session};

    #[tokio::test]
    async fn everyone_nudges_and_admins_switch_it_off() {
        let state = AppState::for_tests().await;
        state
            .guest_mode
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let admin = session(&state, "ana", Role::Admin).await;
        let guest = session(&state, "sam", Role::Guest).await;
        let send = |method, path: &'static str, body: Option<&'static str>, who: String| {
            let state = state.clone();
            async move { call(&state, method, path, body, Some(&who)).await.0 }
        };

        assert_eq!(
            send(
                Method::PUT,
                "/api/mood",
                Some(r#"{"value":1}"#),
                guest.clone()
            )
            .await,
            StatusCode::OK
        );
        let (_, mood) = get_body(&state, "/api/mood", &guest).await;
        let mood: serde_json::Value = serde_json::from_str(&mood).unwrap();
        assert!(mood["value"].as_f64().unwrap() > 0.3, "{mood}");
        assert_eq!(mood["on"], true);
        assert_eq!(
            send(
                Method::PUT,
                "/api/mood",
                Some(r#"{"value":3}"#),
                guest.clone()
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        let off = Some(r#"{"on":false,"strength":50}"#);
        assert_eq!(
            send(Method::PUT, "/api/party/mood", off, guest.clone()).await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            send(Method::PUT, "/api/party/mood", off, admin).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            send(
                Method::PUT,
                "/api/mood",
                Some(r#"{"value":-1}"#),
                guest.clone()
            )
            .await,
            StatusCode::CONFLICT
        );
        let (_, mood) = get_body(&state, "/api/mood", &guest).await;
        assert!(mood.contains(r#""on":false"#), "{mood}");
    }
}
