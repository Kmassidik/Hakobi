//! Login, logout, and session management handlers.

use axum::{
    extract::{Form, State},
    http::{header::ORIGIN, HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use zeroize::Zeroizing;

use super::{
    login_key, origin_is_valid, redirect_response, render_login, CSRF_COOKIE_AGE,
    SESSION_COOKIE_AGE,
};
use crate::utils::random::random_token;
use crate::web::{
    auth::{self, clear_cookie, cookie_value, create_cookie, csrf_is_valid},
    AppState,
};

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
    csrf_token: String,
}

#[derive(Debug, Deserialize)]
pub struct CsrfLogoutForm {
    csrf_token: String,
}

pub(crate) async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<LoginForm>,
) -> Response {
    let origin_valid = origin_is_valid(&headers, &state.config);
    let csrf_valid = csrf_is_valid(&headers, &state.config, &form.csrf_token);
    tracing::debug!(
        origin_valid,
        csrf_valid,
        origin = ?headers.get(ORIGIN),
        csrf_cookie_present = cookie_value(&headers, state.config.csrf_cookie_name()).is_some(),
        submitted_csrf_length = form.csrf_token.len(),
        "login request validation"
    );
    if !origin_valid || !csrf_valid {
        return login_error(
            &state,
            "Your login session expired. Please try again.",
            form.username,
            StatusCode::BAD_REQUEST,
        );
    }
    if form.username.len() > 64 || form.password.len() > 128 {
        return login_error(
            &state,
            "Invalid username or password.",
            form.username,
            StatusCode::UNAUTHORIZED,
        );
    }
    login_validated(&state, &headers, form).await
}

async fn login_validated(state: &AppState, headers: &HeaderMap, form: LoginForm) -> Response {
    let limiter_key = login_key(headers);
    if !state.limiter.allows(&limiter_key).await {
        return login_error(
            state,
            "Too many attempts. Wait five minutes and try again.",
            form.username,
            StatusCode::TOO_MANY_REQUESTS,
        );
    }
    let Some(password_matches) = check_login_password(state, form.password).await else {
        return login_error(
            state,
            "Sign-in is busy. Try again shortly.",
            form.username,
            StatusCode::TOO_MANY_REQUESTS,
        );
    };
    let username_matches = form.username == state.config.username;
    if !password_matches || !username_matches {
        state.limiter.record_failure(&limiter_key).await;
        return login_error(
            state,
            "Invalid username or password.",
            form.username,
            StatusCode::UNAUTHORIZED,
        );
    }
    state.limiter.clear(&limiter_key).await;
    complete_login(state, headers).await
}

fn login_error(state: &AppState, message: &str, username: String, status: StatusCode) -> Response {
    render_login(state, Some(message.into()), username, status, Vec::new())
}

async fn check_login_password(state: &AppState, password: String) -> Option<bool> {
    let _password_permit = state.password_work.clone().try_acquire_owned().ok()?;
    let password = Zeroizing::new(password);
    let password_hash = state.config.password_hash.clone();
    Some(
        tokio::task::spawn_blocking(move || auth::verify_password(&password_hash, &password))
            .await
            .unwrap_or(false),
    )
}

async fn complete_login(state: &AppState, headers: &HeaderMap) -> Response {
    let old_session = cookie_value(headers, state.config.session_cookie_name());
    state.sessions.revoke(old_session).await;
    let session = state.sessions.create().await;
    let csrf = random_token();
    let cookies = vec![
        create_cookie(
            &state.config,
            state.config.session_cookie_name(),
            &session,
            SESSION_COOKIE_AGE,
        ),
        create_cookie(
            &state.config,
            state.config.csrf_cookie_name(),
            &csrf,
            CSRF_COOKIE_AGE,
        ),
    ];
    redirect_response("/", cookies)
}

pub(crate) async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<CsrfLogoutForm>,
) -> Response {
    let session = cookie_value(&headers, state.config.session_cookie_name());
    if !state.sessions.is_valid(session).await {
        return Redirect::to("/login").into_response();
    }
    if !origin_is_valid(&headers, &state.config)
        || !csrf_is_valid(&headers, &state.config, &form.csrf_token)
    {
        return StatusCode::FORBIDDEN.into_response();
    }

    state.sessions.revoke(session).await;
    redirect_response(
        "/login",
        vec![
            clear_cookie(&state.config, state.config.session_cookie_name()),
            clear_cookie(&state.config, state.config.csrf_cookie_name()),
        ],
    )
}
