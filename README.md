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

On staging, open **TEST room** from the tabletop home to try random bot play or a Spy or Swap drill. For a local TEST room, run the server with `CABO_TEST_ROOMS=1`.

### Debug a game

In a TEST room, open **Game log** on the board or a player device, then tap **Copy game log**. Save the copied JSON to a file or paste it into a bug report. It includes accepted and rejected actions, anonymous seat numbers, revisions, and public pile state. The log includes the seed and scenario.

```sh
python3 scripts/check-game-log.py game-log.json
cargo test -p cabo-server seeded_legal_turn_fuzz
```

The checker replays the public trace and reports the first broken invariant. Hidden hands and deck order are excluded, so the trace cannot recreate a normal game's concealed deal. The log retains the latest 1,024 events.

Rooms live in memory. A restart ends active games. Check `/health` for server readiness.

```sh
cargo fmt --all -- --check
cargo test -p cabo-shared -p cabo-server
cargo check -p cabo-app --target wasm32-unknown-unknown
```

Railway builds the root Dockerfile from the `staging` branch. The container serves the client and API from a single service and listens on Railway's `PORT`.
