use std::{env, error::Error};

use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let port: u16 = env::var("PORT").unwrap_or_else(|_| "8080".into()).parse()?;
    let app_dir = env::var("CABO_APP_DIR").unwrap_or_else(|_| "app/dist".into());
    let app = cabo_server::router(&app_dir);
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    println!("Cabo Club listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}
