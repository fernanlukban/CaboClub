# Working in Cabo Club

Read [SPEC.md](SPEC.md) before changing gameplay, hosting, or the client/server protocol. Read the relevant crate before editing and keep changes scoped to the requested behavior.

- Put rendering, input, animation, and client-side interaction state in `app/`. Keep it buildable for `wasm32-unknown-unknown`.
- Put HTTP, sessions, room lifecycle, randomness, state storage, and player-specific view construction in `server/`. Validate each command against current server state and authenticated seat.
- Put deterministic rules, domain types, and public wire types in `shared/`. Keep it usable without Bevy, Axum, networking, or a runtime.
- Render full seat-scoped snapshots and build gameplay controls from `availableActions`. Keep legality, visibility, and state transitions on the server. Use shared rule code to keep action enumeration and command validation aligned.
- Treat the paired board as a public role with `beginPlay` after readiness and deal actions only for its own created room. Build its snapshot on the server and use role-specific fixtures to verify that private card knowledge and player actions never reach it.
- Treat snapshot and command JSON as the frontend compatibility contract. When implementing or changing it, update the spec and canonical fixtures so Bevy and native clients can verify the same actions and payloads.
- Keep round-start transitions in the shared state machine. Check actor, deal, and revision on the server before mutating the room.
- Exercise changes with the narrowest relevant checks below. Add focused coverage for rule changes, turn ownership, redaction, and command replay when those behaviors change.
- Update the spec when a contract or milestone changes. Mark unfinished behavior explicitly and report checks you could not run.

```sh
cargo fmt --all -- --check
cargo test -p cabo-shared -p cabo-server
cargo check -p cabo-app --target wasm32-unknown-unknown
```
