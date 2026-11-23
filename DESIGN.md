# Cabo Club app design

Cabo Club turns an iPad into the shared table and each phone into a player's private hand. A player should be able to join, look at their cards, and take a turn with the same gestures they would use around a physical table. An active board starts each round. Phones can continue playing if it disconnects during a round.

This document specifies presentation and interaction. [SPEC.md](SPEC.md) defines game rules, permissions, and the snapshot contract.

## Visual direction

Use a deep green felt table, warm ivory card faces, muted gold highlights, and large, readable ranks. Card backs have one consistent pattern. Tilt, an edge, and soft contact shadows give the cards depth. Use a gentle idle float with staggered timing. Keep the hit area still during the float.

Use color, shape, and text together. A selected card lifts, gains an outline, and shows a checkmark. A card face and card back remain distinguishable at a glance. Reserve high contrast for the current turn, playable piles, and the primary available action.

## iPad tabletop

The tabletop home has two entry points:

| Entry | What appears |
| --- | --- |
| **Join a game** | Resume the current board session or enter the room and a host-issued board pairing code. Open directly to the current lobby, turn, or results using the latest board snapshot. |
| **Create a new game** | Open a lobby with a large QR code, a short room code for manual entry, and the seats as players join. The QR opens the private player join screen on a phone. |

Place the deck next to the discard pile in the center. Reserve one seat for each cardinal direction. Show each occupied seat’s card backs during play, then show public faces at scoring. Keep seats fixed as hands change and show the name, score, and current-turn indicator beside each hand. Use generous spacing and readable names and values from both sides of the table. During the opening look, show which seats are ready. Once all seats confirm, show **Start game** on the iPad. Later rounds use **Start round**. During play, the iPad displays the current turn, remaining deck, top discard, Cabo final turns, and public results. While a round is playing, the board is a display. It shows public state only. Keep the board route through refresh and reconnect.

If the board disconnects, show a reconnecting state while the phones continue. If the board is revoked, clear the game view and return to tabletop home.

## Landscape phone

Use a compact status strip, a card stage, and an action strip above the bottom safe area. The card stage takes the available space. Respect the cutout and home indicator on either landscape orientation.

| State | Layout and behavior |
| --- | --- |
| Join | Show the room code and a name input. A QR invite preselects the room. |
| Choose opening cards | Show four face-down cards in stable slots. Tap any two, then tap **Reveal 2**. A tap toggles selection until the pair is submitted. |
| Private opening look | Turn over the two selected cards. Keep the other two face down. **I’ve looked** closes the preview and tells the board the seat is ready. |
| Waiting | Show the hand at a comfortable size and a concise turn or readiness indicator. Keep turn actions offscreen. Confirmed opening cards are ordinary card backs. |
| Your turn | Ease out to a wider view, shrink and translate the hand downward, and reveal a second row above it for the deck and discard. Present the server-provided actions beside or below the stage. |
| Resolving a move | Show the incoming card at the edge of the hand. Keep the incoming rank legible. Raise selected hand cards and present only the actions offered for this step. |
| Round or game result | Show publicly revealed hands, points this round, total scores, and winners when available. The host or room-creating board can prepare the next deal. |

Fit the entire current hand without horizontal or vertical scrolling. Size cards from the available stage area and the number of visible slots, including the pending incoming card. Tighten gaps or stagger rows for larger hands while preserving individual targets. Keep stable slot positions during reveals and retain the relative order of cards that remain after exchanges. Compact the affected row only after the move resolves.

A reconnect before **I’ve looked** restores the same private pair. After confirmation, the pair stays hidden across refreshes and retries. The client discards the two displayed ranks and avoids memory aids such as a history strip or remembered rank badges. A valid later reveal or public scoring can show those ranks again.

## Turn gestures

| Physical action | App interaction |
| --- | --- |
| Pull a card off the deck | Drag from the deck toward the hand, or use the equivalent draw button. Show the incoming rank privately. |
| Take the top discard | Tap the discard pile. Animate the card into a temporary hand slot. Tap an existing hand card and choose to discard it, or select multiple hand cards and call a match. |
| Match first | Tap two or more existing hand cards and choose **Call match**. On success, animate those cards onto the discard pile and take the card that was on top before the move. |
| Draw first, then match | Show the drawn card for comparison. Keep the drawn card if the selected hand cards match, and animate every matched card onto the discard pile. Leave the rank comparison and memory work to the player. |
| Use a power | Present selectable targets from the current action descriptor. Privately reveal an authorized Peek or Spy result, or show the swap motion for a confirmed Swap. |
| End a turn | Ease back toward the waiting composition and hand focus. Show the next player on the board. |

The action strip offers accessible tap controls for each gesture. Treat a tap, drag, or button press as the same gameplay command when it expresses the same action and parameters. The client sends intent and animates the accepted result from the server. It can highlight a selection immediately.

## Bevy rendering and motion

Bevy is the intended owner of the game scene and its motion. Model each card as a thin quad or shallow box with separate front, back, and edge materials. Use a restrained perspective camera, soft shadows, and transform animation to get a mostly flat tabletop look with visible dimensionality. Use stable card IDs to reconcile the scene with snapshots. Keep camera position, card transforms, and pointer hit targets in the same scene coordinate system. Reserve HTML for accessible controls, form fields, QR invites, and status text.

- Use ease-in-out for camera translation, zoom, and coordinated card moves. Start with 300–450 ms for camera and card moves, and tune together so the deck appears as the hand settles.
- Tilt and flip around the card's center. Choose the face at the midpoint of a flip using the authorized snapshot. Hidden ranks never enter a hidden card material or accessible label.
- Keep float amplitude small and use a stable hit plane. Cards going into a pile receive a deterministic stacking order. Draws and discards can play a short lift and settle without delaying the next authoritative snapshot.
- On reload, render the newest snapshot immediately. If a new revision arrives during motion, reconcile to it and interrupt obsolete transitions. Reduced-motion mode disables float and uses immediate composition changes and the same controls.

The current implementation is hybrid. The browser uses HTML/CSS for interactive card faces and Bevy for a WebAssembly scene with sprite shadows, camera scale, and eased scene motion. Moving card faces, flips, lighting, and picking into Bevy's scene is the rendering follow-up specified here.

## Browser build and Railway delivery

The Bevy client targets Rust's `wasm32-unknown-unknown`. Trunk builds `app/` and emits browser assets, including a hashed `*_bg.wasm` module and generated JavaScript bindings. The browser loads those bindings and runs Bevy through WebGL2.

Railway uses the root [Dockerfile](Dockerfile) to install the WASM target, run `trunk build --release --locked`, and copy the generated `app/dist/` into the runtime image. The same image runs the native Axum server on Railway's `PORT`. Axum serves the HTML, JavaScript, and WASM and handles the game API on the same origin. The GPU work happens in the browser. The container only builds and serves those assets. The production build therefore needs no GPU.

Verify the platform path before calling the browser build ready:

1. Build the Bevy app with Trunk for the WASM target and verify the generated HTML references a real module.
2. Start the native Axum server with the built assets. Confirm `/health` returns OK, the module is served as `application/wasm`, and the HTML and JS load from the same origin.
3. Run the same checks on the Railway preview. Open the board and phone routes in a WebGL2 browser, then test on an actual iPad and iPhone Safari before claiming physical-device compatibility.

## Interaction checks

- Create on the iPad and scan the QR with two phones. Verify all three views use the same game revision and the board never receives private ranks.
- Reveal two non-adjacent slots, confirm, and reload before starting. Verify both cards stay face down.
- Draw from each pile, match before drawing and after drawing, and verify a three-card match shrinks the hand correctly. Check that a hand still fits after a penalty grows it.
- Switch between waiting and active turn with reduced motion on and off. Check both landscape orientations, an incoming card, larger hands, and independent card selection.
- Pair a board after play starts, disconnect it during a turn, and resume at the current state.
