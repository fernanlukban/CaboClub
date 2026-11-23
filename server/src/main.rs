mod game;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use cabo_shared::protocol::{Command, Snapshot};
use game::{Room, secret};
use qrcode::{QrCode, render::svg};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    error::Error,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::TcpListener, sync::Mutex};
use tower_http::services::ServeDir;

type Rooms = Arc<Mutex<HashMap<String, Room>>>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Create {
    board: bool,
    name: Option<String>,
}

#[derive(Deserialize)]
struct Join {
    name: String,
}

#[derive(Deserialize)]
struct Pair {
    code: String,
}

#[derive(Serialize)]
struct Session {
    token: String,
    snapshot: Snapshot,
}

#[derive(Serialize)]
struct Outcome {
    outcome: &'static str,
    detail: String,
    snapshot: Snapshot,
}

#[derive(Serialize)]
struct Failure {
    error: &'static str,
}

type ApiError = (StatusCode, Json<Failure>);

fn bad(status: StatusCode, error: &'static str) -> ApiError {
    (status, Json(Failure { error }))
}

fn credential(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| bad(StatusCode::UNAUTHORIZED, "invalidSession"))
}

fn room_mut<'a>(
    rooms: &'a mut HashMap<String, Room>,
    code: &str,
) -> Result<&'a mut Room, ApiError> {
    let room = rooms
        .get_mut(&code.to_ascii_uppercase())
        .ok_or_else(|| bad(StatusCode::NOT_FOUND, "roomNotFound"))?;
    if room.last_active.elapsed() > Duration::from_secs(8 * 3600) {
        return Err(bad(StatusCode::NOT_FOUND, "roomNotFound"));
    }
    Ok(room)
}

async fn create(
    State(state): State<Rooms>,
    Json(input): Json<Create>,
) -> Result<Json<Session>, ApiError> {
    let mut rooms = state.lock().await;
    rooms.retain(|_, room| room.last_active.elapsed() < Duration::from_secs(8 * 3600));
    if rooms.len() >= 500 {
        return Err(bad(StatusCode::SERVICE_UNAVAILABLE, "roomCapacity"));
    }
    let code = loop {
        let code = secret()[..8].to_ascii_uppercase();
        if !rooms.contains_key(&code) {
            break code;
        }
    };
    let mut room = Room::new(code.clone(), input.board);
    let token = if input.board {
        room.creator_token().unwrap().to_owned()
    } else {
        room.join(input.name.as_deref().unwrap_or_default())
            .map_err(|err| bad(StatusCode::BAD_REQUEST, err))?
    };
    let snapshot = room.snapshot(room.role(&token).unwrap());
    rooms.insert(code, room);
    Ok(Json(Session { token, snapshot }))
}

async fn join(
    State(state): State<Rooms>,
    Path(code): Path<String>,
    Json(input): Json<Join>,
) -> Result<Json<Session>, ApiError> {
    let mut rooms = state.lock().await;
    let room = room_mut(&mut rooms, &code)?;
    let token = room
        .join(&input.name)
        .map_err(|err| bad(StatusCode::CONFLICT, err))?;
    Ok(Json(Session {
        snapshot: room.snapshot(room.role(&token).unwrap()),
        token,
    }))
}

async fn pair(
    State(state): State<Rooms>,
    Path(code): Path<String>,
    Json(input): Json<Pair>,
) -> Result<Json<Session>, ApiError> {
    let mut rooms = state.lock().await;
    let room = room_mut(&mut rooms, &code)?;
    let token = room
        .pair(&input.code.trim().to_ascii_uppercase())
        .map_err(|err| bad(StatusCode::UNAUTHORIZED, err))?;
    Ok(Json(Session {
        snapshot: room.snapshot(room.role(&token).unwrap()),
        token,
    }))
}

async fn snapshot(
    State(state): State<Rooms>,
    Path(code): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Snapshot>, ApiError> {
    let token = credential(&headers)?;
    let mut rooms = state.lock().await;
    let room = room_mut(&mut rooms, &code)?;
    let role = room
        .role(token)
        .ok_or_else(|| bad(StatusCode::UNAUTHORIZED, "invalidSession"))?;
    room.last_active = Instant::now();
    Ok(Json(room.snapshot(role)))
}

async fn command(
    State(state): State<Rooms>,
    Path(code): Path<String>,
    headers: HeaderMap,
    Json(command): Json<Command>,
) -> Result<Response, ApiError> {
    let token = credential(&headers)?;
    let mut rooms = state.lock().await;
    let room = room_mut(&mut rooms, &code)?;
    let role = room
        .role(token)
        .ok_or_else(|| bad(StatusCode::UNAUTHORIZED, "invalidSession"))?;
    let (status, outcome, detail) = match room.command(token, role, command) {
        Ok(detail) => (StatusCode::OK, "accepted", detail),
        Err(detail) => (StatusCode::CONFLICT, "rejected", detail.into()),
    };
    Ok((
        status,
        Json(Outcome {
            outcome,
            detail,
            snapshot: room.snapshot(role),
        }),
    )
        .into_response())
}

#[derive(Deserialize)]
struct Qr {
    url: String,
}

async fn qr(axum::extract::Query(query): axum::extract::Query<Qr>) -> Result<Response, ApiError> {
    if query.url.len() > 512
        || !(query.url.starts_with("https://") || query.url.starts_with("http://"))
    {
        return Err(bad(StatusCode::BAD_REQUEST, "invalidUrl"));
    }
    let image = QrCode::new(query.url)
        .map_err(|_| bad(StatusCode::BAD_REQUEST, "invalidUrl"))?
        .render::<svg::Color>()
        .min_dimensions(220, 220)
        .build();
    Ok(([(header::CONTENT_TYPE, "image/svg+xml")], image).into_response())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let port: u16 = env::var("PORT").unwrap_or_else(|_| "8080".into()).parse()?;
    let app_dir = env::var("CABO_APP_DIR").unwrap_or_else(|_| "app/dist".into());
    let api = Router::new()
        .route("/rooms", post(create))
        .route("/rooms/{code}", get(snapshot))
        .route("/rooms/{code}/join", post(join))
        .route("/rooms/{code}/board", post(pair))
        .route("/rooms/{code}/commands", post(command))
        .route("/qr", get(qr))
        .layer(axum::extract::DefaultBodyLimit::max(8192))
        .layer(axum::middleware::map_response(
            |mut response: Response| async move {
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
                response
            },
        ))
        .with_state(Rooms::default());
    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .nest("/api", api)
        .fallback_service(ServeDir::new(app_dir));
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
    println!("Cabo Club listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}
