use axum::{
    Json,
    http::StatusCode,
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

pub enum StatusHeaderResponse<T> {
    Success(StatusCode, T),
    Error(StatusCode),
}

impl<T> IntoResponse for StatusHeaderResponse<T>
where
    T: IntoResponse,
{
    fn into_response(self) -> Response {
        match self {
            StatusHeaderResponse::Success(status, response) => (status, response).into_response(),
            StatusHeaderResponse::Error(status) => status.into_response(),
        }
    }
}
