# Cabo Club product and engineering spec

## Goal

Two to four players can open a browser, create a private table or join via an invite, and play a full Cabo game. The host starts the game and opens subsequent rounds. Players keep their seat when reconnecting with a valid session.

## Ownership

- **app/** renders the lobby, table, scores, and results with Bevy. It renders player-specific snapshots, presents the server's enumerated actions, and translates selections into commands.
- **server/** serves the browser build, creates and expires rooms, issues seat credentials, authenticates players, shuffles and deals, advances games, and publishes views. It serializes updates within each room and owns the authoritative game state.
- **shared/** owns pure rule types, move validation, and scoring. Transport-neutral request, snapshot, and action descriptor types belong here when introduced. Full state and player-visible snapshots require distinct types so hidden information cannot accidentally be serialized into client responses.

## Game lifecycle

1. Create a room, return a shareable code and a separate private seat credential, then allow up to four seats while the lobby is open.
2. The host starts with at least two players. Deal four hidden cards per player and privately reveal the first two to their owner. Show the discard pile's top card and the number of cards left in the deck.
3. On a player's turn, allow a legal draw, discard, exchange, special ability, or Cabo call. Apply exchanges and their penalties atomically. Deck cards 7–8 allow Peek, 9–10 allow Spy, and 11–12 allow Swap. A Cabo call grants every other player one final turn. Deck exhaustion ends the round.
4. Reveal hands at round end. A caller tied for the lowest hand scores zero. Otherwise the caller adds ten to their hand total. A hand of 12, 12, 13, 13 scores zero and awards each opponent 50. Exactly 100 resets a player's total to 50 once per game. A total above 100 ends the game and the lowest total wins, with last-round points breaking ties where possible.
5. The host starts the next round or a new game after completion. Preserve the deployed app's detailed match, penalty, turn order, and scoring behavior in regression cases during the port.

## Client/server boundary

The wire contract is versioned JSON. `shared/` will define the Rust types for snapshots, action descriptors, and commands. A native client can implement the same contract directly in Swift. Bevy and native clients own rendering, accessibility, animation, selection, and transport. Game outcomes, visibility, phase transitions, scoring, and available actions come from the server. The server uses shared rules to validate moves and enumerate actions from the same revision.

### Full snapshot for each seat

Create, join, load or resume, poll, and authenticated command responses return a complete snapshot for the requesting seat. Each snapshot can render the current screen by itself. Use the same shape if push updates are added later.

| Snapshot field | Contents |
| --- | --- |
| `protocolVersion`, `roomId`, `revision` | Wire compatibility, room instance identity, and a monotonically increasing room revision |
| `roomCode`, `youSeatId`, `hostSeatId` | Invite code, requesting seat, and host |
| `phase`, `round`, `turnSeatId` | Lobby, active round, round results, or game over, plus current turn when applicable |
| `players`, `table` | Ordered seats, scores, card slots and visible values, deck count, and discard top |
| `pendingMove`, `finalTurns`, `results` | Player-visible draw or power state, Cabo caller and remaining turns, and round or game results as applicable |
| `availableActions` | Actions this seat can submit at this revision, including the choices each action accepts |

Project the authoritative state into a distinct per-seat response type. Include hidden card slots with opaque references so clients can render card backs and select targets where allowed. Reveal only public values and values the requesting seat is entitled to see. Keep deck order, private card values, and seat credentials out of the snapshot. Action descriptors must also preserve hidden information. Send private snapshots with `Cache-Control: no-store`.

Fetch on load or reconnect before enabling gameplay controls, then poll. Apply a snapshot as a replacement for the active room and seat. Ignore responses from previous sessions and snapshots older than the current revision. An equal revision can complete a refresh without resetting local selection. When the revision advances, retain a selection only if the new descriptor still permits it. Keep local pending feedback separate from game state.

### Action catalog

Use stable, tagged action kinds and typed parameters. Each descriptor carries any applicable choice sets, target references, count constraints, and required choices. Use the action kind plus the selected parameters to submit a command. The catalog describes game intent independently of button labels, layout, and input device.

| Action kind | Choices supplied by the server |
| --- | --- |
| `startRound`, `nextRound`, `restartGame` | No parameters. Available only to the host in the applicable phase. |
| `draw` | Currently available piles, such as `deck` or `discard`. |
| `discardDrawn`, `useDrawnPower`, `callCabo` | No parameters. For a power, include the power kind visible to the acting player. |
| `exchange` | Selectable own card slots, minimum and maximum selection counts, and valid insertion sides for a failed exchange when applicable. |
| `peek` | Selectable own card slots. |
| `spy` | Selectable opponent card slots grouped by seat. |
| `swap` | Selectable own slots and opponent slots, with explicit pair constraints if needed. |

Create room, join room, and resume session are entry flows outside the in-room catalog. A seat with no available commands receives an empty array. For an exchange, list selectable slots and count constraints without enumerating matching sets. The server resolves the outcome using hidden card values. Clients can choose their own presentation for waiting and unsupported actions. Unknown action kinds must never be submitted by an older client.

### Command and response flow

1. Submit a unique `commandId`, the snapshot's `expectedRevision`, an action kind, and its selected parameters. Authenticate with the private seat credential. The public room code locates the room.
2. Within one serialized room update, authenticate the seat, check command replay, check the expected revision, then validate the command and parameters against current state. Commit the entire accepted mutation and advance the revision once.
3. Return the command outcome alongside a fresh snapshot projected for that seat. Authenticated stale or illegal commands return a machine-readable rejection and the latest snapshot without changing state. A retry with the same command ID and payload returns the original outcome and a fresh snapshot without replaying the mutation. Reusing an ID with a different payload is rejected.

### Frontend parity

Keep canonical JSON fixtures for lobby, waiting, draw, each card power, round results, and game over when implementing the contract. Include asymmetric visibility fixtures for two seats at the same revision. Each frontend should verify that the fixture produces the same semantic controls and that selections encode the same command payloads. Also exercise direct load into an active game, an empty action list, an unknown action kind, and out-of-order snapshots. Native and browser frontends can use different layouts and gestures.

## Tabletop mode

Tabletop mode is an optional way to play the same room. An iPad lying flat shows the shared table while each player uses a phone in landscape for their private hand. Players can use the regular client view alongside tabletop devices in the same game.

### Pair a board

1. The host opens **Pair a board** on their player device and gets a short-lived, single-use pairing code scoped to the room.
2. On the iPad, choose **Join as board** and enter the pairing code. The server exchanges it for a read-only board session credential. The board can attach during the lobby or an active game.
3. The iPad resumes with its board credential and fetches the latest full board snapshot on load or reconnect. The host can revoke the pairing from their phone. A revoked or expired session returns the iPad to pairing and clears its displayed room state.

The board role uses no player seat and does not count toward the two-to-four-player limit. Pairing and reconnecting leave the turn order and game revision intact. The room can continue if the iPad disconnects. The invite code remains the player join flow.

### What each device shows

| Device | View and controls |
| --- | --- |
| iPad board | Show a large deck with its remaining count, the face-up discard, player names and seat positions, current turn, Cabo final turns, scores, and public round or game results. Make the current player readable from around the table. Show a reconnecting indicator if refresh fails. |
| Player phone | Show the player's four starting slots in a landscape row. Render the actual hand size as exchanges and penalties change it. Show card faces only when the seat snapshot allows them, including private draws, Peek, or Spy. Render the server's available actions and selectable targets, and show enough public context to finish a turn without the iPad. |
| Host phone | Inherits the player view and provides board pairing and revocation in addition to phase-appropriate host controls. |

### Snapshot and permission boundary

Use the same room and revision stream for both roles. Produce a complete public `BoardSnapshot` from the authoritative state on the server. Reuse public wire types from `shared/` and keep private seat fields out of the board response type. The board receives no private hand knowledge, private draw values or power results, seat credentials, or player action descriptors. Card values revealed publicly at scoring may appear on the board. The board credential authorizes board reads only. Authenticate command submissions as player sessions and reject board credentials before command processing.

Phones still receive their normal complete seat snapshots. Landscape layout is a presentation choice. Any Bevy or native client can render the board or a player hand from the relevant role's snapshot.

### Acceptance

- Pair an iPad and join on two phones. Both phones and the board can refresh into the same current round without depending on previously received updates.
- At the same revision, the board shows the public turn and piles. Only the authorized phone sees its private draw or power result. A board credential cannot read a seat view or submit a player command.
- Rotate a phone to landscape, carry out a turn, then reload mid-round. The hand and available controls are restored from the latest seat snapshot.
- Disconnect or revoke the board while the phones continue playing. Reconnection restores public state with a valid board session. Revocation requires a fresh pairing.

## Hosting

Use one Railway service on `PORT` for the compiled Bevy client and API. Keep `GET /health` independent of game traffic. For the first playable version, scope in-memory rooms to one replica, cap room creation, expire inactive rooms, and disclose that restarts end active games. Durable games and multiple replicas are later work.

## Implementation milestones

- **Scaffold** — Bevy demo, Axum static hosting and health route, and a shared crate with draw validation.
- **Lobby** — Create, join, resume, and start rooms. Acceptance requires two independent browser sessions to see the same lobby while keeping seat credentials private.
- **Playable round** — Port turns, card actions, and scoring. Acceptance requires a full two-player round, rejected illegal or stale moves, and verified player-specific redaction.
- **Full game** — Port round progression and end-game behavior. Acceptance requires reconnect during play, safe command retry, and a complete game from invite to final scores.
- **Tabletop mode** — Add board pairing, the public board snapshot, and the landscape hand view. Verify the same room from an iPad and two independent phone sessions.
