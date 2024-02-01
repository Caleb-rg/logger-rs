use actix_web::get;
use actix_web::post;
use actix_web::web;
use actix_web::web::Data;
use actix_web::App;
use actix_web::HttpResponse;
use actix_web::HttpServer;
use actix_web::Responder;
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

#[actix_web::main]
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

    println!("Connecting to database: {connection}...");

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
    let res = HttpServer::new(move || {
        App::new()
            .app_data(Data::new(AppState { db: db.clone() }))
            .service(index)
            .service(log)
            .service(giveme)
    })
    .bind((host, port))?
    .run()
    .await;

    if let Err(err) = res {
        eprintln!("Error: {err}");
        return Ok(());
    }

    Ok(())
}

#[derive(Deserialize)]
struct Log {
    name: String,
    data: HashMap<String, serde_json::Value>,
}

#[get("/")]
async fn index() -> impl Responder {
    HttpResponse::Ok()
}

#[post("/log")]
async fn log(req_body: web::Json<Log>, state: web::Data<AppState>) -> impl Responder {
    if let Err(err) = sqlx::query("INSERT INTO logs VALUES($1, $2, $3, $4)")
        .bind(Uuid::new_v4())
        .bind(&req_body.name)
        .bind(json!(req_body.data))
        .bind(chrono::Utc::now())
        .execute(&*state.db.borrow())
        .await
    {
        eprintln!("Error: {err}");
        return HttpResponse::InternalServerError().body("Could not log data");
    }

    HttpResponse::Ok().body("Success")
}

#[derive(Debug, Deserialize)]
pub struct GivemeRequest {
    key: Option<String>,
    all: Option<bool>,
}

#[get("/giveme")]
async fn giveme(query: web::Query<GivemeRequest>, state: web::Data<AppState>) -> impl Responder {
    let key = std::env::var("KEY").unwrap_or("x".to_string());
    let limit = std::env::var("LIMIT")
        .map(|l| l.parse::<i64>().ok())
        .unwrap_or(Some(100))
        .unwrap();

    if query.key.is_none() || query.key.as_ref().unwrap() != key.as_str() {
        return HttpResponse::Unauthorized().body("Nope");
    }

    let query = if query.all.unwrap_or(false) {
        "SELECT * FROM logs order by created desc".to_string()
    } else {
        format!("SELECT * FROM logs order by created desc limit {limit}")
    };

    sqlx::query(&query)
        .fetch_all(&*state.db.borrow())
        .await
        .map(|res| {
            let mut data = Vec::new();

            for row in res {
                data.push(json!({
                    "id": row.get::<Uuid, _>("id").to_string(),
                    "name": row.get::<String, _>("name"),
                    "data": row.get::<serde_json::Value, _>("data"),
                    "created": row.get::<chrono::DateTime<chrono::Utc>, _>("created"),
                }));
            }

            HttpResponse::Ok().json(json!({
                "data": data
            }))
        })
        .unwrap_or(HttpResponse::InternalServerError().body("Could not get data"))
}
