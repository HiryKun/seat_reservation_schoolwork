use super::StatusJsonResponse;
use axum::{
    Json,
    extract::{Extension, State},
    http::StatusCode,
    response::IntoResponse,
};
use bcrypt::verify;
use chrono::{Duration, Local};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use tracing::{debug, error, warn};
use uuid::Uuid;

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct UserInfoResponse {
    username: String,
    score: i32,
}

/* 用户登录 */
pub async fn login(
    State(pool): State<MySqlPool>,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    if payload.username.trim().is_empty() || payload.password.trim().is_empty() {
        return StatusJsonResponse::Error(StatusCode::BAD_REQUEST);
    }

    match sqlx::query!(
        "SELECT id, password_hash FROM users WHERE id = ?",
        payload.username
    )
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(record)) => {
            if verify(payload.password, &record.password_hash).unwrap_or(false) {
                ()
            } else {
                return StatusJsonResponse::Error(StatusCode::UNAUTHORIZED);
            }
        }
        Ok(None) => {
            warn!("用户 {} 尝试登录失败", payload.username);
            return StatusJsonResponse::Error(StatusCode::UNAUTHORIZED);
        }
        Err(e) => {
            error!("数据库查询失败 {}", e);
            return StatusJsonResponse::Error(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    let token = Uuid::new_v4().to_string();
    let expires_at = (Local::now().naive_local() + Duration::hours(24))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();

    match sqlx::query!(
        "UPDATE users SET token = ?, expires_at = ? WHERE id = ?",
        token,
        expires_at,
        payload.username
    )
    .execute(&pool)
    .await
    {
        Ok(_) => (),
        Err(e) => {
            error!("数据库查询失败 {}", e);
            return StatusJsonResponse::Error(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }
    debug!("用户 {} 登录成功", payload.username);
    StatusJsonResponse::Success(
        StatusCode::OK,
        serde_json::json!({
            "token": token,
            "expires_at": expires_at,
        }),
    )
}

/* 获取用户资料 */
pub async fn get_user_profile(
    State(pool): State<MySqlPool>,
    Extension(user_id): Extension<String>,
) -> impl IntoResponse {
    let record = match sqlx::query!(
        "
        SELECT name, score
        FROM users
        WHERE id = ?",
        user_id
    )
    .fetch_one(&pool)
    .await
    {
        Ok(record) => record,
        Err(e) => {
            error!("数据库查询失败 {}", e);
            return StatusJsonResponse::Error(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let user_profile = UserInfoResponse {
        username: record.name,
        score: record.score,
    };

    debug!(
        "用户 {} 积分：{}",
        user_profile.username, user_profile.score
    );

    StatusJsonResponse::Success(StatusCode::OK, user_profile)
}
