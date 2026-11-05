use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use super::FileSharingState;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub role: String,
    pub jti: String,
    pub exp: usize,
}

/// 生成 token 及其会话 id
pub fn create_token(
    user_id: &str,
    username: &str,
    role: &str,
    secret: &str,
    duration_hours: u64,
) -> Result<(String, String), String> {
    let expiration = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(duration_hours as i64))
        .expect("valid timestamp")
        .timestamp() as usize;

    let token_id = uuid::Uuid::new_v4().to_string();
    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        role: role.to_string(),
        jti: token_id.clone(),
        exp: expiration,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map(|token| (token, token_id))
    .map_err(|e| e.to_string())
}

pub fn validate_token(token: &str, secret: &str) -> Result<Claims, String> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|e| e.to_string())
}

pub fn hash_password(password: &str) -> Result<String, String> {
    bcrypt::hash(password, bcrypt::DEFAULT_COST).map_err(|e| e.to_string())
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool, String> {
    bcrypt::verify(password, hash).map_err(|e| e.to_string())
}

pub async fn auth_middleware(
    State(state): State<Arc<FileSharingState>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // <img>/<video>/<iframe>/<a download> 无法附加请求头，这些场景用 ?token= 传递
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .or_else(|| {
            request.uri().query().and_then(|query| {
                query.split('&').find_map(|pair| {
                    let value = pair.strip_prefix("token=")?;
                    Some(
                        urlencoding::decode(value)
                            .map(|v| v.into_owned())
                            .unwrap_or_else(|_| value.to_string()),
                    )
                })
            })
        });

    let token = match token {
        Some(t) => t,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    let config = state.config.read().await;
    let secret = config.jwt_secret.clone();
    drop(config);

    match validate_token(&token, &secret) {
        Ok(claims) => {
            let now = Instant::now();
            let mut sessions = state.sessions.lock().await;
            sessions.retain(|_, session| session.expires_at > now);
            match sessions.get_mut(&claims.jti) {
                Some(session) => session.last_active = now,
                // 会话已被强制下线或服务重启，token 即使未过期也不再放行
                None => return Err(StatusCode::UNAUTHORIZED),
            }
            drop(sessions);
            request.extensions_mut().insert(claims);
            Ok(next.run(request).await)
        }
        Err(_) => Err(StatusCode::UNAUTHORIZED),
    }
}
