use super::StatusJsonResponse;
use super::floors::AvailabilityQuery;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{Local, NaiveDate};
use serde::Serialize;
use sqlx::MySqlPool;
use tracing::{debug, error, warn};

#[derive(Serialize)]
struct SeatAvailability {
    slot_id: String,
    status: String,
}

/* 查询座位状态 */
pub async fn get_seat_availability(
    State(pool): State<MySqlPool>,
    Path(seat_id): Path<String>,
    Query(params): Query<AvailabilityQuery>,
) -> impl IntoResponse {
    if seat_id.trim().is_empty() {
        warn!("传入的 seat_id 为空");
        return StatusJsonResponse::Error(StatusCode::BAD_REQUEST);
    }

    let date = match params.date {
        Some(d) => {
            if let Err(_) = NaiveDate::parse_from_str(&d, "%Y-%m-%d") {
                warn!("传入的日期不合规：{}", d);
                return StatusJsonResponse::Error(StatusCode::BAD_REQUEST);
            }
            d
        }
        None => {
            let date = Local::now().format("%Y-%m-%d").to_string();
            warn!("未传入查询日期，默认为 {}", date);
            date
        }
    };

    match sqlx::query!(
        "
        SELECT s.id AS slot_id, IF(r.id IS NOT NULL, 'OCCUPIED', 'AVAILABLE') AS status
        FROM `slots` s
        LEFT JOIN `reservations` r 
        ON s.id = r.slot_id 
        AND r.seat_id = ?
        AND r.date = ?
        AND r.status IN ('PENDING', 'ACTIVE', 'SUSPENDED')
        ORDER BY s.start_time ASC",
        seat_id,
        date
    )
    .fetch_all(&pool)
    .await
    {
        Ok(record) => {
            let availability: Vec<SeatAvailability> = record
                .into_iter()
                .map(|record| SeatAvailability {
                    slot_id: record.slot_id,
                    status: record.status,
                })
                .collect();
            debug!("查询座位信息成功：{}", seat_id);
            StatusJsonResponse::Success(StatusCode::OK, availability)
        }
        Err(e) => {
            error!("数据库查询失败 {}", e);
            StatusJsonResponse::Error(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
