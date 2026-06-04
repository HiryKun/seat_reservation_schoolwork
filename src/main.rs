use axum::{
    Router, middleware,
    routing::{get, post},
};
use sqlx::MySqlPool;
use tower_http::trace::TraceLayer;
use tracing::info;
mod handlers;
mod my_middleware;
mod tasks;
use handlers::{floors, reserve, seats, slots, users};
use my_middleware::auth;

const DB_URL: &str = "mysql://seat_res:123456@localhost/seat_res_db";
const LISTENING_ADDR: &str = "0.0.0.0:8080";

#[tokio::main]
async fn main() {
    /* 初始化日志系统，默认从环境变量读取日志级别配置 */
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    /* 启动定时任务 */
    let pool = MySqlPool::connect(DB_URL).await.unwrap();
    info!("数据库连接成功");

    /* 启动定时任务 */
    tokio::spawn(tasks::start_deadline_checker(pool.clone()));
    tokio::spawn(tasks::start_database_cleaner(pool.clone()));

    /* 需要验证身份的路由 */
    let protected = Router::new()
        .route("/floors", get(floors::get_floors_list))
        .route("/floors/{floor_id}/layout", get(floors::get_floor_layout))
        .route(
            "/floors/{floor_id}/availability",
            get(floors::get_floor_availability),
        )
        .route("/slots", get(slots::get_slots_list))
        .route(
            "/seats/{seat_id}/availability",
            get(seats::get_seat_availability),
        )
        .route("/reservations", post(reserve::create_reservation))
        .route("/reservations/me", get(reserve::get_reservation_list))
        .route("/user/profile", get(users::get_user_profile))
        .route(
            "/reservations/{reservation_id}/check-in",
            post(reserve::check_in),
        )
        .route(
            "/reservations/{reservation_id}/suspend",
            post(reserve::suspend),
        )
        .route(
            "/reservations/{reservation_id}/finish",
            post(reserve::finish),
        )
        .route(
            "/reservations/{reservation_id}/cancel",
            post(reserve::cancel),
        )
        .route_layer(middleware::from_fn_with_state(
            pool.clone(),
            auth::auth_middleware,
        ))
        .with_state(pool.clone());

    /* 主路由 */
    let app = Router::new()
        .route("/auth/login", post(users::login))
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .with_state(pool.clone());

    /* 启动服务 */
    let listener = tokio::net::TcpListener::bind(LISTENING_ADDR).await.unwrap();
    info!("监听在 {}", LISTENING_ADDR);
    axum::serve(listener, app).await.unwrap();
}
