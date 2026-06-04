use super::StatusJsonResponse;
use axum::{extract::State, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use sqlx::MySqlPool;
use tracing::{debug, error};

#[derive(Serialize)]
struct Slot {
    id: String,
    name: String,
    start: String,
    end: String,
}

/* 获取时段信息 */
pub async fn get_slots_list(State(pool): State<MySqlPool>) -> impl IntoResponse {
    match sqlx::query!(
        "
        SELECT id, name, start_time, end_time 
        FROM slots 
        ORDER BY start_time ASC"
    )
    .fetch_all(&pool)
    .await
    {
        Ok(record) => {
            let slots: Vec<Slot> = record
                .into_iter()
                .map(|record| Slot {
                    id: record.id,
                    name: record.name,
                    start: record.start_time.format("%H:%M:%S").to_string(),
                    end: record.end_time.format("%H:%M:%S").to_string(),
                })
                .collect();
            debug!("获取时段列表成功");
            StatusJsonResponse::Success(StatusCode::OK, slots)
        }
        Err(e) => {
            error!("数据库查询失败 {}", e);
            StatusJsonResponse::Error(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
