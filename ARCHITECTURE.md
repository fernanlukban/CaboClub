# Cabo Club architecture

[Product rules and wire behavior](SPEC.md) · [Interaction and motion design](DESIGN.md)

## Workspace boundaries

| Crate | Owns | Internal dependencies |
| --- | --- | --- |
| `cabo-shared` | Public wire types, scoring and deterministic round-start rules | None |
| `cabo-room` | Authoritative room state, session roles, legal actions, command replay, game transitions, and role-scoped snapshots | `cabo-shared` |
| `cabo-server` | HTTP extraction and responses, room lookup and expiry, serialized room access, tick scheduling, static assets, and process configuration | `cabo-room`, `cabo-shared` |
| `cabo-app` | Bevy scenes, visual selection and transitions, browser bindings, and client command construction | `cabo-shared` |

Keep Bevy and browser APIs in the app. Keep Axum and the Tokio runtime in the HTTP host. Keep pure rules in shared so the room and test tools can invoke them without either runtime. The room crate remains under `server/` because it owns private game state and never ships to clients.

## Room state and HTTP

- `server/room/src/lib.rs` owns the private room model, sessions, action eligibility, and command dispatch. The command path uses the same action descriptors that appear in snapshots. The HTTP layer resolves the role from the supplied credential while holding the room lock. Callers must use that authenticated role for both command execution and the response snapshot.
- `server/room/src/turn.rs` owns dealing, turn mutations, matching and scoring orchestration. For a rule that can be expressed with pure arguments, put the calculation in `shared/` and invoke it here.
- `server/room/src/view.rs` constructs snapshots for a role. Keep opening looks, Spy results, and face-down ranks scoped here. If a new snapshot field exposes game state, check both its owner view and the public tabletop view, including retries or reconnects.
- The host creates the router on a Tokio runtime, which also runs the room ticker. Storage is currently an in-memory map behind one lock. If contention calls for finer-grained locks, preserve one mutation order per room across both commands and ticks. Room expiration and credential behavior need coverage before changing storage.

## Client rendering and input

- `app/src/lib.rs` starts Bevy and selects the scene. `flat.rs` owns the legacy DOM-aligned sprite layer.
- `scene3d/frame.rs` defines the renderer's read model. It reuses the shared command, action descriptors, and card-travel receipt. The read model takes the subset of the snapshot needed for rendering, so it can evolve independently of unused server fields.
- `scene3d/layout.rs` turns that read model into card placements for the phone and tabletop. `scene3d.rs` reconciles the placements with Bevy entities, advances animations, and picks against the live mesh. Texture generation lives in `scene3d/textures.rs`.
- `scene3d/browser.rs` owns WebAssembly exports, selection scoping, and camera projection for browser controls. `app/club.js` owns session polling, optimistic requests, and semantic HTML. When 3D is enabled, the camera projection supplies the DOM card bounds and label anchors. Keep keyboard controls and their focus indicators aligned with the same animated geometry.

## Changes that cross boundaries

- **A new action** starts with a shared wire descriptor. The room must check its current eligibility and targets. Clients may use the published descriptors to guide selection.
- **A public animation** starts from the accepted room transition and needs a stable identity in the snapshot when viewers must see it across polls. Scope playback to the room, deal, and revision. Repeated polls should settle into the same scene. Late or reconnecting clients must be able to build the current scene from the snapshot.
- **A local selection** belongs to the presentation layer. When an uncommitted Spy target changes, the old card returns home before the new card moves into focus. Once the player confirms, only a seat-scoped server result can authorize the private face reveal.
- **A future renderer migration** should preserve a semantic control and inspection surface for automation. Exercise protocol behavior with deterministic room tests and verify geometry, animation order, and keyboard interaction in the browser. Keep any synthetic controls or test-only snapshots out of public gameplay.

## Where to split next

The browser controller currently handles transport, optimistic state, and HTML in one file. If it grows, separate request and session ownership from DOM rendering first. Keep one owner for the accepted snapshot and pending command so a late poll cannot undo a newer selection or replay a transition. Move animation logic into a separate crate only when another host or a headless verifier needs to consume it. Until then, the frame-to-placement module is the boundary for testing layout without starting WebGL.
