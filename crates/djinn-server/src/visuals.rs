//! Visualizers for node screens: which styles are in the rotation, how they
//! take turns, and how each one looks. Set for the whole house here on Main
//! (Admin > Visualizers); a node can have its own instead (`nodes.visual`).
//! Nodes are sent what applies to them in each check-in and draw it
//! themselves.

use std::collections::BTreeMap;

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Setting key: the house's visualizers, as JSON.
pub const VISUALS: &str = "visuals";

/// Every style a node can draw: shapes on any box, then the ones drawn on
/// the graphics chip.
pub const STYLES: &[&str] = &[
    "bars",
    "ring",
    "mirror",
    "wave",
    "spectrogram",
    "scope",
    "mountain",
    "vu",
    "circle",
    "particles",
    "terrain",
    "flyover",
    "warp",
    "tunnel",
    "plasma",
    "kaleidoscope",
    "nebula",
    "stars",
];

/// How the styles in the rotation take turns.
const CHANGES: &[&str] = &["song", "time", "never"];
const COLORS: &[&str] = &["default", "rainbow"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Visuals {
    /// The styles in the rotation, in order.
    pub styles: Vec<String>,
    /// "song" (the next one with each song), "time" (every `every_secs`) or
    /// "never" (only the first).
    pub change: String,
    pub every_secs: u32,
    /// How each style looks, where it isn't the default.
    #[serde(default)]
    pub looks: BTreeMap<String, Look>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Look {
    /// How strongly it reacts, 0.25 to 3.
    pub intensity: f32,
    /// "default" or "rainbow" (colors turning slowly).
    pub colors: String,
}

impl Default for Visuals {
    fn default() -> Self {
        Visuals {
            styles: ["spectrogram", "bars", "wave", "ring", "plasma", "tunnel"]
                .map(str::to_owned)
                .to_vec(),
            change: "song".to_owned(),
            every_secs: 60,
            looks: BTreeMap::new(),
        }
    }
}

impl Visuals {
    /// Refuses what a node couldn't draw.
    pub fn check(&self) -> ApiResult<()> {
        let known = |style: &String| STYLES.contains(&style.as_str());
        if self.styles.is_empty() || !self.styles.iter().all(known) {
            return Err(ApiError::bad_request(
                "pick at least one visualizer DJINN knows",
            ));
        }
        if self
            .styles
            .iter()
            .enumerate()
            .any(|(i, style)| self.styles[..i].contains(style))
        {
            return Err(ApiError::bad_request("a visualizer is listed twice"));
        }
        if !CHANGES.contains(&self.change.as_str()) || !(5..=3600).contains(&self.every_secs) {
            return Err(ApiError::bad_request(
                "visualizers change with each song, every 5 seconds to an hour, or never",
            ));
        }
        for (style, look) in &self.looks {
            if !known(style)
                || !(0.25..=3.0).contains(&look.intensity)
                || !COLORS.contains(&look.colors.as_str())
            {
                return Err(ApiError::bad_request("a visualizer's look is out of range"));
            }
        }
        Ok(())
    }
}

/// The house's visualizers (the default until an admin saves some).
pub async fn house(state: &AppState) -> ApiResult<Visuals> {
    Ok(state
        .store
        .get_setting(VISUALS)
        .await?
        .and_then(|json| serde_json::from_str::<Visuals>(&json).ok())
        .filter(|visuals| visuals.check().is_ok())
        .unwrap_or_default())
}

pub fn router() -> Router<AppState> {
    Router::new().route("/visuals", get(get_visuals).put(put_visuals))
}

async fn get_visuals(_: AdminUser, State(state): State<AppState>) -> ApiResult<Json<Visuals>> {
    Ok(Json(house(&state).await?))
}

async fn put_visuals(
    _: AdminUser,
    State(state): State<AppState>,
    Json(visuals): Json<Visuals>,
) -> ApiResult<Json<Visuals>> {
    visuals.check()?;
    state
        .store
        .set_setting(VISUALS, &serde_json::to_string(&visuals)?)
        .await?;
    Ok(Json(visuals))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, session};
    use axum::http::{Method, StatusCode};
    use djinn_store::Role;

    #[test]
    fn only_drawable_visuals_pass() {
        assert!(Visuals::default().check().is_ok());
        let with = |change: fn(&mut Visuals)| {
            let mut visuals = Visuals::default();
            change(&mut visuals);
            visuals.check().is_ok()
        };
        assert!(!with(|v| v.styles.clear()));
        assert!(!with(|v| v.styles.push("lasers".into())));
        assert!(!with(|v| v.styles.push("bars".into())));
        assert!(!with(|v| v.change = "sometimes".into()));
        assert!(!with(|v| v.every_secs = 1));
        assert!(!with(|v| {
            v.looks.insert(
                "bars".into(),
                Look {
                    intensity: 9.0,
                    colors: "default".into(),
                },
            );
        }));
        assert!(with(|v| {
            v.looks.insert(
                "plasma".into(),
                Look {
                    intensity: 1.5,
                    colors: "rainbow".into(),
                },
            );
        }));
    }

    #[tokio::test]
    async fn admins_set_the_house_visuals() {
        let state = AppState::for_tests().await;
        let admin = session(&state, "ana", Role::Admin).await;
        let user = session(&state, "bo", Role::User).await;
        let (status, _) = call(&state, Method::GET, "/api/visuals", None, Some(&user)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let visuals = Visuals {
            styles: vec!["vu".into(), "stars".into()],
            change: "time".into(),
            ..Visuals::default()
        };
        let body = serde_json::to_string(&visuals).unwrap();
        let (status, _) = call(
            &state,
            Method::PUT,
            "/api/visuals",
            Some(&body),
            Some(&admin),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(house(&state).await.unwrap(), visuals);
    }
}
