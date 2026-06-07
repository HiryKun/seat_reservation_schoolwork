use axum::{
    Json,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};

pub enum StatusJsonResponse<T> {
    Success(StatusCode, T),
    Error(StatusCode),
}

impl<T> IntoResponse for StatusJsonResponse<T>
where
    T: serde::Serialize,
{
    fn into_response(self) -> Response {
        match self {
            StatusJsonResponse::Success(status, json) => (status, Json(json)).into_response(),
            StatusJsonResponse::Error(status) => status.into_response(),
        }
    }
}

pub enum StatusHeaderResponse {
    // 成功时：只需传入状态码，和跳转的 URL 路径
    Success(StatusCode, String),
    Error(StatusCode),
}

impl IntoResponse for StatusHeaderResponse {
    fn into_response(self) -> Response {
        match self {
            StatusHeaderResponse::Success(status, location_path) => {
                let headers = [(header::LOCATION, location_path)];
                (status, headers, ()).into_response()
            }
            StatusHeaderResponse::Error(status) => status.into_response(),
        }
    }
}
