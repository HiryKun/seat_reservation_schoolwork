use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::MySqlPool;
use tracing::{debug, error, warn};

use super::StatusJsonResponse;

#[derive(Serialize)]
struct FloorLayout {
    floor: Floor,
    canvas_size: CanvasSize,
    seats: Vec<Seat>,
}

#[derive(Serialize)]
struct CanvasSize {
    width: i32,
    height: i32,
}

#[derive(Serialize, Deserialize)]
struct Seat {
    id: String,
    name: String,
    x: i32,
    y: i32,
}

#[derive(Serialize)]
struct Floor {
    id: String,
    name: String,
}

#[derive(Deserialize)]
pub struct AvailabilityQuery {
    pub date: Option<String>,
    pub slot_id: Option<String>,
}

#[derive(Serialize)]
struct SeatAvailability {
    seat_id: String,
    slot_id: String,
    status: String,
}

/* 获取楼层列表 */
pub async fn get_floors_list(State(pool): State<MySqlPool>) -> impl IntoResponse {
    match sqlx::query!(
        "
        SELECT id, name 
        FROM floors 
        ORDER BY id ASC"
    )
    .fetch_all(&pool)
    .await
    {
        Ok(record) => {
            let floors: Vec<Floor> = record
                .into_iter()
                .map(|record| Floor {
                    id: record.id,
                    name: record.name,
                })
                .collect();
            debug!("查询楼层列表成功");
            StatusJsonResponse::Success(StatusCode::OK, floors)
        }
        Err(e) => {
            error!("数据库查询失败 {}", e);
            StatusJsonResponse::Error(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/* 获取楼层布局 */
pub async fn get_floor_layout(
    State(pool): State<MySqlPool>,
    Path(floor_id): Path<String>,
) -> impl IntoResponse {
    if floor_id.trim().is_empty() {
        warn!("传入的 floor_id 为空");
        return StatusJsonResponse::Error(StatusCode::BAD_REQUEST);
    }

    let layout_data = match sqlx::query!(
        "
        SELECT 
            f.id AS floor_id,
            f.name AS floor_name,
            f.width,
            f.height,
            (
                SELECT JSON_ARRAYAGG(
                    JSON_OBJECT('id', s.id, 'name', s.name, 'x', s.x, 'y', s.y)
                )
                FROM (SELECT * FROM seats WHERE floor_id = ? ORDER BY id ASC) s
            ) AS seats_json
        FROM floors f
        WHERE f.id = ?",
        floor_id,
        floor_id
    )
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(record)) => record,
        Ok(None) => {
            warn!("请求的 floor_id 不存在：{}", floor_id);
            return StatusJsonResponse::Error(StatusCode::NOT_FOUND);
        }
        Err(e) => {
            error!("数据库查询失败 {}", e);
            return StatusJsonResponse::Error(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let seats: Vec<Seat> = if let Some(json_str) = layout_data.seats_json {
        serde_json::from_str(&json_str).unwrap_or_default()
    } else {
        Vec::new()
    };

    let floor_layout = FloorLayout {
        floor: Floor {
            id: layout_data.floor_id,
            name: layout_data.floor_name,
        },
        canvas_size: CanvasSize {
            width: layout_data.width,
            height: layout_data.height,
        },
        seats,
    };
    debug!("查询 {} 的布局", floor_id);
    StatusJsonResponse::Success(StatusCode::OK, floor_layout)
}

/* 查询楼层座位占用总览 */
pub async fn get_floor_availability(
    State(pool): State<MySqlPool>,
    Path(floor_id): Path<String>,
    Query(params): Query<AvailabilityQuery>,
) -> impl IntoResponse {
    if floor_id.trim().is_empty() {
        warn!("传入的 floor_id 为空");
        return StatusJsonResponse::Error(StatusCode::BAD_REQUEST);
    }

    let date = match params.date {
        Some(d) => match NaiveDate::parse_from_str(&d, "%Y-%m-%d") {
            Ok(parsed_date) => {
                debug!("查询的日期为 {}", parsed_date);
                parsed_date
            }
            Err(_) => {
                warn!("传入的日期不合规：{}", d);
                return StatusJsonResponse::Error(StatusCode::BAD_REQUEST);
            }
        },
        None => {
            let date = Local::now().date_naive();
            warn!("未传入查询日期，默认为 {}", date);
            date
        }
    };

    let record_result = sqlx::query!(
        "
        SELECT
            s.id AS seat_id,
            CAST(COALESCE(?, (SELECT id FROM slots LIMIT 1)) AS CHAR) AS slot_id,
            IF(r.id IS NULL, 'AVAILABLE', 'OCCUPIED') AS status
        FROM seats s
        LEFT JOIN reservations r ON s.id = r.seat_id 
            AND r.date = ? 
            AND r.slot_id = COALESCE(?, (SELECT id FROM slots LIMIT 1))
            AND r.status IN ('PENDING', 'ACTIVE', 'SUSPENDED')
        WHERE s.floor_id = ? 
        ORDER BY s.id ASC",
        params.slot_id,
        date,
        params.slot_id,
        floor_id
    )
    .fetch_all(&pool)
    .await;

    match record_result {
        Ok(records) => {
            let availability: Vec<SeatAvailability> = records
                .into_iter()
                .map(|rec| SeatAvailability {
                    seat_id: rec.seat_id,
                    slot_id: rec.slot_id.expect("数据库错误"),
                    status: rec.status,
                })
                .collect();
            debug!("查询 {} 的状态图成功", floor_id);
            StatusJsonResponse::Success(StatusCode::OK, availability)
        }
        Err(e) => {
            error!("数据库查询失败 {}", e);
            StatusJsonResponse::Error(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
