use super::{StatusHeaderResponse, StatusJsonResponse};
use axum::{
    Json,
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{Duration, Local, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use tracing::{debug, error, warn};
use uuid::Uuid;

const SCORE_LIMIT: i32 = 10;
const CHECKIN_PERIOD_MINUTES: i64 = 10;
const PERSONAL_MAX_RESERVATIONS: i64 = 1;
const EARLIST_RESERVE_PERIOD_HOURS: i64 = 24;
const SUSPEND_MAX_TIME_MINUTES: i64 = 40;

#[derive(Deserialize)]
pub struct ReservationRequest {
    seat_id: String,
    date: String,
    slot_id: String,
}

#[derive(Deserialize)]
pub struct ReservationListQuery {
    pub status: Option<String>,
}

#[derive(Serialize)]
struct ReservationResponse {
    id: String,
    seat_id: String,
    seat_name: String,
    date: NaiveDate,
    slot_id: String,
    slot_name: String,
    status: String,
    next_deadline: String,
    note: Option<String>,
}

struct ReservationInfo {
    pub user_id: String,
    pub status: String,
    pub date: NaiveDate,
    pub start_time: NaiveTime,
    pub end_time: NaiveTime,
}

/* 创建预约 */
pub async fn create_reservation(
    State(pool): State<MySqlPool>,
    Extension(user_id): Extension<String>,
    Json(payload): Json<ReservationRequest>,
) -> impl IntoResponse {
    if payload.seat_id.trim().is_empty()
        || payload.slot_id.trim().is_empty()
        || payload.date.trim().is_empty()
    {
        warn!("传入有空字符");
        return StatusHeaderResponse::Error(StatusCode::BAD_REQUEST);
    }

    let Ok(date) = NaiveDate::parse_from_str(&payload.date, "%Y-%m-%d") else {
        warn!("传入日期不合规：{}", payload.date);
        return StatusHeaderResponse::Error(StatusCode::BAD_REQUEST);
    };
    let current_date = Local::now().date_naive();
    if date < current_date {
        warn!("不能创建过期的预约：{}", date);
        return StatusHeaderResponse::Error(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let validation = match sqlx::query!("
        SELECT 
            u.score AS user_score,
            s.start_time AS slot_start,
            s.end_time AS slot_end,
            (SELECT COUNT(*) FROM reservations r WHERE r.user_id = u.id AND r.status IN ('PENDING', 'ACTIVE', 'SUSPENDED')) AS active_res_count
        FROM users u
        CROSS JOIN slots s
        WHERE u.id = ? AND s.id = ?",
        user_id, payload.slot_id
    )
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            warn!("用户或时段不存在：{} {}", user_id, payload.slot_id);
            return StatusHeaderResponse::Error(StatusCode::NOT_FOUND)
        },
        Err(e) => {
            error!("数据库查询失败 {}", e);
            return StatusHeaderResponse::Error(StatusCode::INTERNAL_SERVER_ERROR)
        },
    };

    if date == current_date && validation.slot_end < Local::now().time() {
        warn!(
            "时段已过期：{} 结束于 {}",
            payload.slot_id, validation.slot_end
        );
        return StatusHeaderResponse::Error(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let earliest_res_time =
        date.and_time(validation.slot_start) - Duration::hours(EARLIST_RESERVE_PERIOD_HOURS);
    if earliest_res_time > Local::now().naive_local() {
        warn!(
            "还没到可预约时间：{} 在 {} 后可预约",
            payload.slot_id, earliest_res_time
        );
        return StatusHeaderResponse::Error(StatusCode::UNPROCESSABLE_ENTITY);
    }

    if validation.user_score < SCORE_LIMIT {
        warn!("积分不足：{}", validation.user_score);
        return StatusHeaderResponse::Error(StatusCode::FORBIDDEN);
    }

    if let Some(count) = validation.active_res_count {
        if count >= PERSONAL_MAX_RESERVATIONS {
            warn!("超过预约数量上限：{}", count);
            return StatusHeaderResponse::Error(StatusCode::FORBIDDEN);
        }
    }

    let reservation_id = Uuid::new_v4().to_string();
    let base_time = std::cmp::max(
        date.and_time(validation.slot_start),
        Local::now().naive_local(),
    );
    let checkin_deadline = base_time + Duration::minutes(CHECKIN_PERIOD_MINUTES);
    match sqlx::query!(
        "
        INSERT INTO reservations (id, user_id, seat_id, date, slot_id, next_deadline, status)
        VALUES (?, ?, ?, ?, ?, ?, 'PENDING')",
        reservation_id,
        user_id,
        payload.seat_id,
        date,
        payload.slot_id,
        checkin_deadline
    )
    .execute(&pool)
    .await
    {
        Ok(_) => {
            debug!("创建预约成功：{}", reservation_id);
            StatusHeaderResponse::Success(
                StatusCode::CREATED,
                format!("Location: /reservations/{}", reservation_id),
            )
        }
        Err(e) => {
            warn!("此座位已被预约：{}", e);
            StatusHeaderResponse::Error(StatusCode::CONFLICT)
        }
    }
}

/* 查询我的预约列表 */
pub async fn get_reservation_list(
    State(pool): State<MySqlPool>,
    Extension(user_id): Extension<String>,
    Query(params): Query<ReservationListQuery>,
) -> impl IntoResponse {
    let db_records = match sqlx::query!(
        r#"
        SELECT 
            r.id,
            s.id AS seat_id,
            s.name AS seat_name,
            r.date,
            sl.id AS slot_id,
            sl.name AS slot_name,
            CAST(r.status AS CHAR) AS "status: String",
            r.next_deadline,
            r.note
        FROM reservations r
        JOIN seats s ON r.seat_id = s.id
        JOIN slots sl ON r.slot_id = sl.id
        WHERE r.user_id = ?
          AND (? IS NULL OR r.status = ?)
        ORDER BY r.date DESC, sl.start_time DESC"#,
        user_id,
        params.status,
        params.status
    )
    .fetch_all(&pool)
    .await
    {
        Ok(records) => records,
        Err(e) => {
            error!("数据库查询失败 {}", e);
            return StatusJsonResponse::Error(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let reservations: Vec<ReservationResponse> = db_records
        .into_iter()
        .map(|row| ReservationResponse {
            id: row.id,
            seat_id: row.seat_id,
            seat_name: row.seat_name,
            date: row.date,
            slot_id: row.slot_id,
            slot_name: row.slot_name,
            status: row.status.unwrap_or_default(),
            next_deadline: row.next_deadline.format("%Y-%m-%d %H:%M:%S").to_string(),
            note: row.note,
        })
        .collect();
    debug!("用户 {} 查询预约列表成功", user_id);
    StatusJsonResponse::Success(StatusCode::OK, reservations)
}

/* 验证预约记录是否存在，并属于本人 */
async fn validate_reservation(
    pool: &MySqlPool,
    reservation_id: &str,
    user_id: &str,
) -> Result<ReservationInfo, StatusCode> {
    let record = match sqlx::query_as!(
        ReservationInfo,
        r#"
        SELECT s.user_id AS user_id, s.status AS status, s.date AS date, sl.start_time AS start_time, sl.end_time AS end_time
        FROM reservations s
        INNER JOIN slots sl ON s.slot_id = sl.id 
        WHERE s.id = ?
        "#,
        reservation_id
    )
    .fetch_one(pool)
    .await
    {
        Ok(record) => record,
        Err(_) => {
            warn!("预约记录不存在：{}", reservation_id);
            return Err(StatusCode::NOT_FOUND);
        }
    };

    if record.user_id != user_id {
        warn!("预约发起人与请求用户不一致：{} {}", record.user_id, user_id);
        return Err(StatusCode::FORBIDDEN);
    }
    debug!("存在与权限检查通过：{} {}", user_id, reservation_id);
    Ok(record)
}

/* 更新数据库预约状态 */
async fn update_status(
    pool: &MySqlPool,
    reservation_id: &str,
    status: &str,
    deadline: NaiveDateTime,
) -> StatusCode {
    match sqlx::query!(
        r#"
        UPDATE reservations
        SET status = ?, next_deadline = ?
        WHERE id = ?"#,
        status,
        deadline,
        reservation_id
    )
    .execute(pool)
    .await
    {
        Ok(_) => {
            debug!(
                "更新状态成功：{} TO {} EXP_AT {}",
                reservation_id, status, deadline
            );
            return StatusCode::OK;
        }
        Err(e) => {
            error!("数据库查询失败 {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
}

/* 签到 */
pub async fn check_in(
    State(pool): State<MySqlPool>,
    Path(reservation_id): Path<String>,
    Extension(user_id): Extension<String>,
) -> impl IntoResponse {
    let record = match validate_reservation(&pool, &reservation_id, &user_id).await {
        Ok(record) => record,
        Err(status_code) => return status_code,
    };

    let checkin_time = record.date.and_time(record.start_time);
    if checkin_time > Local::now().naive_local() {
        warn!("未到签到时间：{}", checkin_time);
        return StatusCode::UNPROCESSABLE_ENTITY;
    }

    match record.status.as_str() {
        "PENDING" | "SUSPENDED" => {
            let deadline = record.date.and_time(record.end_time);
            return update_status(&pool, &reservation_id, "ACTIVE", deadline).await;
        }
        "ACTIVE" | "COMPLETED" | "CANCELLED" => return StatusCode::CONFLICT,
        e => {
            error!("数据库异常 status = {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
}

/* 暂离 */
pub async fn suspend(
    State(pool): State<MySqlPool>,
    Path(reservation_id): Path<String>,
    Extension(user_id): Extension<String>,
) -> impl IntoResponse {
    let record = match validate_reservation(&pool, &reservation_id, &user_id).await {
        Ok(record) => record,
        Err(status_code) => return status_code,
    };

    match record.status.as_str() {
        "ACTIVE" => {
            let deadline = Local::now().naive_local() + Duration::minutes(SUSPEND_MAX_TIME_MINUTES);
            return update_status(&pool, &reservation_id, "SUSPENDED", deadline).await;
        }
        "PENDING" | "SUSPENDED" | "COMPLETED" | "CANCELLED" => return StatusCode::CONFLICT,
        e => {
            error!("数据库异常 status = {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
}

/* 结束占用 */
pub async fn finish(
    State(pool): State<MySqlPool>,
    Path(reservation_id): Path<String>,
    Extension(user_id): Extension<String>,
) -> impl IntoResponse {
    let record = match validate_reservation(&pool, &reservation_id, &user_id).await {
        Ok(record) => record,
        Err(status_code) => return status_code,
    };

    match record.status.as_str() {
        "ACTIVE" | "SUSPENDED" => {
            let deadline = Local::now().naive_local();
            return update_status(&pool, &reservation_id, "COMPLETED", deadline).await;
        }
        "PENDING" | "COMPLETED" | "CANCELLED" => return StatusCode::CONFLICT,
        e => {
            error!("数据库异常 status = {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
}

/* 取消预约 */
pub async fn cancel(
    State(pool): State<MySqlPool>,
    Path(reservation_id): Path<String>,
    Extension(user_id): Extension<String>,
) -> impl IntoResponse {
    let record = match validate_reservation(&pool, &reservation_id, &user_id).await {
        Ok(record) => record,
        Err(status_code) => return status_code,
    };

    match record.status.as_str() {
        "PENDING" => {
            let deadline = Local::now().naive_local();
            return update_status(&pool, &reservation_id, "CANCELLED", deadline).await;
        }
        "ACTIVE" | "SUSPENDED" | "COMPLETED" | "CANCELLED" => return StatusCode::CONFLICT,
        e => {
            error!("数据库异常 status = {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    }
}
