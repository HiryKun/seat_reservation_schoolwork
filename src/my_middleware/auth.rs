use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use chrono::Local;
use sqlx::MySqlPool;

/* 验证用户身份 */
pub async fn auth_middleware(
    State(pool): State<MySqlPool>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let user_id = match sqlx::query!(
        "SELECT id FROM users WHERE token = ? AND expires_at > ?",
        token,
        Local::now().naive_local()
    )
    .fetch_one(&pool)
    .await
    {
        Ok(record) => record.id,
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };

    request.extensions_mut().insert(user_id);

    Ok(next.run(request).await)
}
