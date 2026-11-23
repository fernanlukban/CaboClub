# Cabo Club

Cabo Club is a Rust web app for playing Cabo with friends. The Bevy browser client and Axum server support private in-memory rooms and tabletop play. See [SPEC.md](SPEC.md) for the multiplayer milestones and the full-snapshot and action contract shared by browser and future native clients. See [DESIGN.md](DESIGN.md) for device layouts, motion, and Bevy WebAssembly delivery.

[Tabletop mode](SPEC.md#tabletop-mode) supports iPad room creation and resume, QR joins, and private hands on landscape phones.

| Crate | Responsibility |
| --- | --- |
| `app/` | Bevy WebAssembly client |
| `server/` | Axum HTTP server, static hosting, and room and session management |
| `shared/` | Pure Rust game and protocol code for both targets |

## Develop locally

Install Rust with the WebAssembly target and Trunk:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked --version 0.21.14
```

Build the client and run the same-origin server from the repository root.

```sh
(cd app && trunk build --locked)
cargo run --locked -p cabo-server
```

Open http://localhost:8080/tabletop on the tabletop device. Create a room and open its invite in two independent browser sessions. Once both players join, deal cards, have each player reveal and confirm any two cards, then start play from the board. For physical phones, use the deployed URL or the server machine's reachable LAN address instead of localhost.

Rooms live in memory. A restart ends active games. Check `/health` for server readiness.

```sh
cargo fmt --all -- --check
cargo test -p cabo-shared -p cabo-server
cargo check -p cabo-app --target wasm32-unknown-unknown
```

Railway builds the root Dockerfile from the `staging` branch. The container serves the client and API from a single service and listens on Railway's `PORT`.
