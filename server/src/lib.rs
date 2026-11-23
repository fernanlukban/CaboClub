use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use cabo_room::{Room, secret};
use cabo_shared::protocol::{Command, Direction, Snapshot, TestScenario};
use qrcode::{QrCode, render::svg};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tower_http::services::{ServeDir, ServeFile};

type Rooms = Arc<Mutex<HashMap<String, Room>>>;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Create {
    board: bool,
    name: Option<String>,
    direction: Option<Direction>,
    test_scenario: Option<TestScenario>,
    seed: Option<String>,
}

#[derive(Deserialize)]
struct Join {
    name: String,
    direction: Option<Direction>,
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
    if room.last_active().elapsed() > ROOM_TTL {
        return Err(bad(StatusCode::NOT_FOUND, "roomNotFound"));
    }
    Ok(room)
}

fn test_rooms_enabled() -> bool {
    env::var("CABO_TEST_ROOMS").map_or_else(
        |_| env::var("RAILWAY_ENVIRONMENT_NAME").is_ok_and(|value| value == "staging"),
        |value| value == "1",
    )
}

async fn capabilities() -> Json<[&'static str; 1]> {
    Json([if test_rooms_enabled() {
        "testRooms"
    } else {
        ""
    }])
}

async fn create(
    State(state): State<Rooms>,
    Json(input): Json<Create>,
) -> Result<Json<Session>, ApiError> {
    let mut rooms = state.lock().await;
    rooms.retain(|_, room| room.last_active().elapsed() < ROOM_TTL);
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
    if let Some(scenario) = input.test_scenario {
        if !input.board || !test_rooms_enabled() {
            return Err(bad(StatusCode::FORBIDDEN, "testRoomsDisabled"));
        }
        let seed = input
            .seed
            .filter(|value| !value.is_empty())
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(|_| bad(StatusCode::BAD_REQUEST, "invalidSeed"))?
            .unwrap_or_else(rand::random);
        room.enable_test(scenario, seed);
    }
    let token = if input.board {
        room.creator_token().unwrap().to_owned()
    } else {
        room.join(input.name.as_deref().unwrap_or_default(), input.direction)
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
        .join(&input.name, input.direction)
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
    room.touch();
    Ok(Json(room.snapshot(role)))
}

async fn game_log(
    State(state): State<Rooms>,
    Path(code): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let token = credential(&headers)?;
    let mut rooms = state.lock().await;
    let room = room_mut(&mut rooms, &code)?;
    if room.role(token).is_none() {
        return Err(bad(StatusCode::UNAUTHORIZED, "invalidSession"));
    }
    Ok(Json(room.game_log()).into_response())
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

const ROOM_TTL: Duration = Duration::from_secs(8 * 3600);

/// Builds the HTTP service and starts its room tick loop on the active Tokio runtime.
pub fn router(app_dir: &str) -> Router {
    let rooms = Rooms::default();
    let ticking_rooms = rooms.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(200));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let now = Instant::now();
            for room in ticking_rooms.lock().await.values_mut() {
                if now.saturating_duration_since(room.last_active()) < ROOM_TTL {
                    room.tick(now);
                }
            }
        }
    });
    let api = Router::new()
        .route("/capabilities", get(capabilities))
        .route("/rooms", post(create))
        .route("/rooms/{code}", get(snapshot))
        .route("/rooms/{code}/join", post(join))
        .route("/rooms/{code}/board", post(pair))
        .route("/rooms/{code}/commands", post(command))
        .route("/rooms/{code}/log", get(game_log))
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
        .with_state(rooms);
    let app_shell = std::path::Path::new(app_dir).join("index.html");
    Router::new()
        .route_service("/tabletop", ServeFile::new(&app_shell))
        .route_service("/tabletop/", ServeFile::new(&app_shell))
        .route("/health", get(|| async { "ok" }))
        .nest("/api", api)
        .fallback_service(ServeDir::new(app_dir))
}
