use axum::extract::Query;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::routing::post;
use axum::Json;
use axum::Router;
use deadpool_diesel::postgres::Manager;
use deadpool_diesel::postgres::Pool;
use diesel::prelude::*;
use diesel::table;
use dotenv::dotenv;
use serde::Deserialize;
use serde_json::json;
use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::env;
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_PORT: u16 = 8080;

#[derive(Clone)]
struct EnvVars {
    key: Arc<String>,
    limit: i64,
}

thread_local! {
    static VARS: RefCell<EnvVars> = RefCell::new(EnvVars {
        key: Arc::new(std::env::var("KEY").unwrap_or("x".to_string())),
        limit: std::env::var("LIMIT").map(|l| l.parse::<i64>().ok()).unwrap_or(Some(100)).unwrap(),
    });
}

struct AppState {
    db: Arc<Pool>,
}

table! {
    logs (id) {
        id -> Uuid,
        name -> Text,
        data -> Jsonb,
        created -> Timestamptz,
    }
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

    let manager = Manager::new(connection, deadpool_diesel::Runtime::Tokio1);
    let db = Pool::builder(manager).max_size(4).build();

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
        .with_state(Arc::new(AppState { db }));

    let listener = tokio::net::TcpListener::bind(&format!("{host}:{port}"))
        .await
        .unwrap();

    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("Runtime: {err}");
        return Ok(());
    }

    Ok(())
}

#[derive(Identifiable, Queryable, Selectable, Eq, PartialEq)]
#[diesel(table_name = logs)]
struct Log {
    id: Uuid,
    name: String,
    data: serde_json::Value,
    created: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
struct StrippedLog {
    name: String,
    data: serde_json::Value,
}

async fn index() -> Json<serde_json::Value> {
    Json(json!({
        "message": ":)"
    }))
}

async fn log(
    State(state): State<Arc<AppState>>,
    Json(req_body): Json<StrippedLog>,
) -> Json<serde_json::Value> {
    use logs::dsl;

    let conn = state.db.clone().borrow_mut().get().await.unwrap();
    let query = conn
        .interact(move |conn| {
            diesel::insert_into(dsl::logs)
                .values((
                    dsl::id.eq(Uuid::new_v4()),
                    dsl::name.eq(req_body.name.clone()),
                    dsl::data.eq(json!(req_body.data)),
                    dsl::created.eq(chrono::Utc::now()),
                ))
                .execute(conn)
        })
        .await;

    if let Err(err) = query {
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
    use self::logs::dsl::*;

    let EnvVars { key, limit } = VARS.with(|vars| vars.borrow().clone());

    if query.key.is_none() || query.key.as_ref().unwrap() != key.as_str() {
        return Json(
            json!({ "status": StatusCode::UNAUTHORIZED.as_u16(), "message": "Unauthorized" }),
        );
    }

    let query = state
        .db
        .clone()
        .borrow_mut()
        .get()
        .await
        .unwrap()
        .interact(move |conn| {
            if query.all.unwrap_or(false) {
                logs.select(Log::as_select()).load(conn)
            } else {
                logs.limit(limit).select(Log::as_select()).load(conn)
            }
        })
        .await;

    if let Err(err) = query {
        eprintln!("Error: {err}");
        return Json(
            json!({ "status": StatusCode::INTERNAL_SERVER_ERROR.as_u16(), "message": "Could not get data" }),
        );
    }

    let res = query.unwrap().unwrap();
    let mut response = Vec::<serde_json::Value>::with_capacity(res.len());

    for log in res.into_iter() {
        response.push(json!({
            "id": log.id.to_string(),
            "name": log.name,
            "data": log.data,
            "created": log.created,
        }));
    }

    Json(json!({ "status": StatusCode::OK.as_u16(), "message": "OK", "data": response }))
}
