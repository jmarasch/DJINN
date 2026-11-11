//! The house's default language: what every app shows unless the person
//! has picked their own (Admin > System). Empty means each device's own
//! language. Public, on both ports, since apps ask before anyone signs in.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::auth::AdminUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Setting key: a language code ("es", "fr"), or empty.
pub const DEFAULT_LANGUAGE: &str = "default_language";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Language {
    /// A language code, or empty for each device's own.
    pub default: String,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/language", get(get_language).put(put_language))
}

async fn get_language(State(state): State<AppState>) -> ApiResult<Json<Language>> {
    Ok(Json(Language {
        default: state
            .store
            .get_setting(DEFAULT_LANGUAGE)
            .await?
            .unwrap_or_default(),
    }))
}

/// A language code as the apps name their translation files: two or three
/// lowercase letters, maybe a region ("pt-br").
fn looks_like_a_language(code: &str) -> bool {
    let mut parts = code.split('-');
    let base = parts.next().unwrap_or("");
    let region = parts.next();
    (2..=3).contains(&base.len())
        && base.chars().all(|c| c.is_ascii_lowercase())
        && region.is_none_or(|r| {
            (2..=4).contains(&r.len()) && r.chars().all(|c| c.is_ascii_alphanumeric())
        })
        && parts.next().is_none()
}

async fn put_language(
    _: AdminUser,
    State(state): State<AppState>,
    Json(language): Json<Language>,
) -> ApiResult<Json<Language>> {
    if !language.default.is_empty() && !looks_like_a_language(&language.default) {
        return Err(ApiError::bad_request("that isn't a language code"));
    }
    state
        .store
        .set_setting(DEFAULT_LANGUAGE, &language.default)
        .await?;
    Ok(Json(language))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{call, get_body, session};
    use axum::http::{Method, StatusCode};
    use djinn_store::Role;

    #[tokio::test]
    async fn anyone_reads_the_default_and_admins_set_it() {
        let state = AppState::for_tests().await;
        let (status, body) = get_body(&state, "/api/language", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"default":""}"#);
        let user = session(&state, "bo", Role::User).await;
        let admin = session(&state, "ana", Role::Admin).await;
        let spanish = Some(r#"{"default":"es"}"#);
        let (status, _) = call(&state, Method::PUT, "/api/language", spanish, Some(&user)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = call(&state, Method::PUT, "/api/language", spanish, Some(&admin)).await;
        assert_eq!(status, StatusCode::OK);
        let (_, body) = get_body(&state, "/api/language", "").await;
        assert_eq!(body, r#"{"default":"es"}"#);
        let bad = Some(r#"{"default":"<script>"}"#);
        let (status, _) = call(&state, Method::PUT, "/api/language", bad, Some(&admin)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn language_codes() {
        for good in ["es", "fr", "pt-br", "zh-hans", "ast"] {
            assert!(looks_like_a_language(good), "{good}");
        }
        for bad in ["", "e", "ES", "spanish", "es-", "es-br-x", "../x"] {
            assert!(!looks_like_a_language(bad), "{bad}");
        }
    }
}
