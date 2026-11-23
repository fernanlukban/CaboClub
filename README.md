# Cabo Club

A Rust and Bevy browser starter with an interactive demo table. Gameplay, room management, and scoring from the production app still need to be ported.

## Run locally

Install Rust and Trunk, then run:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked --version 0.21.14
trunk serve
```

Open http://localhost:8080 and click the table or press Space to flip the demo cards.

## Deploy to Railway

Add this repository and the `staging` branch as a GitHub service in the staging environment. Railway uses the Dockerfile to build the WebAssembly client and serve it through nginx. The service listens on Railway's `PORT` and provides a health check at `/health`.
