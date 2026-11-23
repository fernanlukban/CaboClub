# Cabo Club

Cabo Club is a Rust web app for playing Cabo with friends. The Bevy browser demo and Axum hosting scaffold are in place. See [SPEC.md](SPEC.md) for the multiplayer milestones and the full-snapshot and action contract shared by browser and future native clients.

[Tabletop mode](SPEC.md#tabletop-mode) is planned for iPad room creation and resume, QR joins, and private hands on landscape phones.

| Crate | Responsibility |
| --- | --- |
| `app/` | Bevy WebAssembly client |
| `server/` | Axum HTTP server, static hosting, and future room and session management |
| `shared/` | Pure Rust game and protocol code for both targets |

## Develop locally

Install Rust with the WebAssembly target and Trunk:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked --version 0.21.14
```

For a fast client preview, run `cd app && trunk serve`. For same-origin hosting, run `(cd app && trunk build)` from the repository root, then run `cargo run -p cabo-server` and open http://localhost:8080. Check `/health` for server readiness.

```sh
cargo fmt --all -- --check
cargo test -p cabo-shared -p cabo-server
cargo check -p cabo-app --target wasm32-unknown-unknown
```

Railway builds the root Dockerfile from the `staging` branch. The container serves the client and API from a single service and listens on Railway's `PORT`.
