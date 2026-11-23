use std::{env, error::Error};

use axum::{Router, routing::get};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let port: u16 = env::var("PORT").unwrap_or_else(|_| "8080".into()).parse()?;
    let app_dir = env::var("CABO_APP_DIR").unwrap_or_else(|_| "app/dist".into());
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .nest("/api", Router::new().fallback(|| async { axum::http::StatusCode::NOT_FOUND }))
        .fallback_service(ServeDir::new(app_dir));
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    println!("Cabo Club listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}
