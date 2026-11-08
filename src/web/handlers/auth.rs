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
        return render_login(
            &state,
            Some("Your login session expired. Please try again.".into()),
            form.username,
            StatusCode::BAD_REQUEST,
            Vec::new(),
        );
    }
    if form.username.len() > 64 || form.password.len() > 128 {
        return render_login(
            &state,
            Some("Invalid username or password.".into()),
            form.username,
            StatusCode::UNAUTHORIZED,
            Vec::new(),
        );
    }

    let limiter_key = login_key(&headers);
    if !state.limiter.allows(&limiter_key).await {
        return render_login(
            &state,
            Some("Too many attempts. Wait five minutes and try again.".into()),
            form.username,
            StatusCode::TOO_MANY_REQUESTS,
            Vec::new(),
        );
    }

    let Ok(_password_permit) = state.password_work.clone().try_acquire_owned() else {
        return render_login(
            &state,
            Some("Sign-in is busy. Try again shortly.".into()),
            form.username,
            StatusCode::TOO_MANY_REQUESTS,
            Vec::new(),
        );
    };

    let password = Zeroizing::new(form.password);
    let password_hash = state.config.password_hash.clone();
    let password_matches =
        tokio::task::spawn_blocking(move || auth::verify_password(&password_hash, &password))
            .await
            .unwrap_or(false);
    let username_matches = form.username == state.config.username;

    if !password_matches || !username_matches {
        state.limiter.record_failure(&limiter_key).await;
        return render_login(
            &state,
            Some("Invalid username or password.".into()),
            form.username,
            StatusCode::UNAUTHORIZED,
            Vec::new(),
        );
    }

    state.limiter.clear(&limiter_key).await;
    let old_session = cookie_value(&headers, state.config.session_cookie_name());
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
