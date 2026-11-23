# Working in Cabo Club

Read [SPEC.md](SPEC.md) before changing gameplay, hosting, or the client/server protocol. Read the relevant crate before editing and keep changes scoped to the requested behavior.

- Put rendering, input, animation, and client-side interaction state in `app/`. Keep it buildable for `wasm32-unknown-unknown`.
- Put HTTP, sessions, room lifecycle, randomness, state storage, and player-specific view construction in `server/`. Validate each command against current server state and authenticated seat.
- Put deterministic rules, domain types, and public wire types in `shared/`. Keep it usable without Bevy, Axum, networking, or a runtime.
- Avoid duplicating rules in UI systems or request handlers. Client checks may use only the facts available in that player's snapshot.
- Exercise changes with the narrowest relevant checks below. Add focused coverage for rule changes, turn ownership, redaction, and command replay when those behaviors change.
- Update the spec when a contract or milestone changes. Mark unfinished behavior explicitly and report checks you could not run.

```sh
cargo fmt --all -- --check
cargo test -p cabo-shared -p cabo-server
cargo check -p cabo-app --target wasm32-unknown-unknown
```
