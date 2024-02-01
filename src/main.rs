use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use dotenv::dotenv;
use serde::Deserialize;
use serde_json::json;
use sqlx::postgres::PgPoolOptions;
use sqlx::types::chrono;
use sqlx::Row;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_PORT: u16 = 8080;

#[derive(Clone)]
struct AppState {
    db: Arc<sqlx::PgPool>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    if let Ok(value) = std::env::var("RUST_LOG") {
        if value == "debug" {
            env_logger::init();
        }
    }

    let host = env::var("HOST").unwrap_or("localhost".to_string());
    let port = env::var("PORT")
        .map(|p| p.parse::<u16>().unwrap_or(DEFAULT_PORT))
        .unwrap_or(DEFAULT_PORT);

    let connection = format!(
        "postgres://{}:{}@{}:{}/{}",
        env::var("DB_USER").unwrap_or("postgres".to_string()),
        env::var("DB_PASSWORD").unwrap_or("postgres".to_string()),
        env::var("DB_HOST").unwrap_or("localhost".to_string()),
        env::var("DB_PORT").unwrap_or("5432".to_string()),
        env::var("DB_NAME").unwrap_or("postgres".to_string()),
    );

    println!("Connecting to database...");

    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&connection)
        .await;

    if let Err(err) = db {
        eprintln!("Error: {err}");
        return Ok(());
    }

    println!("Connected to database");

    let db = Arc::new(db.unwrap());
    let app = Router::new()
        .route("/", get(index))
        .route("/log", post(log))
        .route("/giveme", get(giveme))
        .with_state(Arc::new(AppState { db: db.clone() }));

    let listener = tokio::net::TcpListener::bind(&format!("{host}:{port}"))
        .await
        .unwrap();

    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("Runtime: {err}");
        return Ok(());
    }

    Ok(())
}

#[derive(Deserialize)]
struct Log {
    name: String,
    data: HashMap<String, serde_json::Value>,
}

async fn index() -> Json<serde_json::Value> {
    Json(json!({
        "message": ":)"
    }))
}

async fn log(
    State(state): State<Arc<AppState>>,
    Json(req_body): Json<Log>,
) -> Json<serde_json::Value> {
    if let Err(err) = sqlx::query("INSERT INTO logs VALUES($1, $2, $3, $4)")
        .bind(Uuid::new_v4())
        .bind(&req_body.name)
        .bind(json!(req_body.data))
        .bind(chrono::Utc::now())
        .execute(&*state.db.borrow())
        .await
    {
        eprintln!("Error: {err}");
        return Json(
            json!({ "status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(), "message": "Could not log data" }),
        );
    }

    Json(json!({ "status": 200, "message": "OK" }))
}

#[derive(Debug, Deserialize)]
pub struct GivemeRequest {
    key: Option<String>,
    all: Option<bool>,
}

async fn giveme(
    Query(query): Query<GivemeRequest>,
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let key = std::env::var("KEY").unwrap_or("x".to_string());
    let limit = std::env::var("LIMIT")
        .map(|l| l.parse::<i64>().ok())
        .unwrap_or(Some(100))
        .unwrap();

    if query.key.is_none() || query.key.as_ref().unwrap() != key.as_str() {
        return Json(
            json!({ "status": StatusCode::UNAUTHORIZED.as_u16(), "message": "Unauthorized" }),
        );
    }

    let query = if query.all.unwrap_or(false) {
        "SELECT * FROM logs order by created desc".to_string()
    } else {
        format!("SELECT * FROM logs order by created desc limit {limit}")
    };

    let query = sqlx::query(&query).fetch_all(&*state.db.borrow()).await;

    if let Err(_) = query {
        return Json(
            json!({ "status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(), "message": "Could not get data" }),
        );
    }

    let res = query.unwrap();
    let mut data = Vec::<serde_json::Value>::with_capacity(res.len());

    for row in res {
        data.push(json!({
            "id": row.get::<Uuid, _>("id").to_string(),
            "name": row.get::<String, _>("name"),
            "data": row.get::<serde_json::Value, _>("data"),
            "created": row.get::<chrono::DateTime<chrono::Utc>, _>("created"),
        }));
    }

    println!("Finished building data with {} entries", data.len());

    Json(json!({ "status": StatusCode::OK.as_u16(), "message": "OK", "data": data }))
}
