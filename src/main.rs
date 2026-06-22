use axum::{
    Router,
    http::{Method, header},
    middleware,
    routing::{get, post},
};
use sqlx::MySqlPool;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::info;
mod handlers;
mod my_middleware;
mod tasks;
use handlers::{floors, reserve, seats, slots, users};
use my_middleware::auth;

#[tokio::main]
async fn main() {
    /* 初始化日志系统，默认从环境变量读取日志级别配置 */
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    /* 创建数据库连接池 */
    print!("请输入数据库连接信息，格式为 mysql://用户名:密码@主机/数据库名\n");
    print!("默认 mysql://seat_res:123456@localhost/seat_res_db\n");
    let mut db_url = String::new();
    std::io::stdin().read_line(&mut db_url).unwrap();
    let db_url = match db_url.trim() {
        "" => "mysql://seat_res:123456@localhost/seat_res_db",
        url => url,
    };
    let pool = MySqlPool::connect(db_url).await.unwrap();
    info!("数据库连接成功在 {}", db_url);

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

    /* CORS 跨域配置：前端 IP 不固定，放行所有源；按需求限定方法与请求头 */
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::OPTIONS,
            Method::PUT,
            Method::DELETE,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

    /* 主路由 */
    let app = Router::new()
        .route("/auth/login", post(users::login))
        .merge(protected)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(pool.clone());

    /* 启动服务 */
    print!("请输入监听地址，格式为 IP:端口，默认 0.0.0.0:8080\n");
    let mut listening_addr = String::new();
    std::io::stdin().read_line(&mut listening_addr).unwrap();
    let listening_addr = match listening_addr.trim() {
        "" => "0.0.0.0:8080",
        addr => addr,
    };
    let listener = tokio::net::TcpListener::bind(listening_addr).await.unwrap();
    info!("监听在 {}", listening_addr);
    axum::serve(listener, app).await.unwrap();
}
