# Cabo Club product and engineering spec

## Goal

Two to four players can open a browser, create a private table or join via an invite, and play a full Cabo game. The host deals rounds. A paired board or the host begins play once every player confirms their opening look. Players keep their seat when reconnecting with a valid session.

## Ownership

- **app/** renders the lobby, table, scores, and results with Bevy. It renders player-specific snapshots, presents the server's enumerated actions, and translates selections into commands.
- **server/** serves the browser build, creates and expires rooms, issues seat credentials, authenticates players, shuffles and deals, advances games, and publishes views. It serializes updates within each room and owns the authoritative game state.
- **shared/** owns pure rule types, move validation, and scoring. Transport-neutral request, snapshot, and action descriptor types belong here when introduced. Full state and player-visible snapshots require distinct types so hidden information cannot accidentally be serialized into client responses.

## Game lifecycle

1. Create a room, return a shareable code and a separate private seat credential, then allow up to four seats while the lobby is open.
2. The host deals four hidden cards per player with at least two players present. Each player privately chooses two of their four cards to look at and confirms before the board or host begins play. Show the discard pile's top card and the number of cards left in the deck.
3. On a player's turn, allow a legal draw, discard, exchange, special ability, or Cabo call. Apply exchanges and their penalties atomically. Deck cards 7–8 allow Peek, 9–10 allow Spy, and 11–12 allow Swap. A Cabo call grants every other player one final turn. Deck exhaustion ends the round.
4. Reveal hands at round end. A caller tied for the lowest hand scores zero. Otherwise the caller adds ten to their hand total. A hand of 12, 12, 13, 13 scores zero and awards each opponent 50. Exactly 100 resets a player's total to 50 once per game. A total above 100 ends the game and the lowest total wins, with last-round points breaking ties where possible.
5. The host deals the next round or a new game after completion. Preserve the deployed app's detailed match, penalty, turn order, and scoring behavior in regression cases during the port.

## Round-start state machine

The host deals and the table starts play in two separate steps. Use the same flow for the first and subsequent rounds, with or without a board.

| State | Event and guard | Result |
| --- | --- | --- |
| `lobby`, `roundResults`, or `gameOver` | Host selects **Deal cards**, **Deal next round**, or **New game**, respectively. Require two to four seats. | Freeze the round roster, deal once, reset readiness, and enter `initialPreview`. There is no active turn yet. |
| `initialPreview` | A seated player chooses two distinct slots from their own four starting cards and taps **Reveal 2**. | Record the selected pair for this deal and reveal those two values in the seat's private snapshot. A reconnect before confirmation restores the same pair. Further reveal requests cannot change it. |
| `initialPreview` | The same player selects **I've looked** after revealing. | Record readiness and hide that seat's opening preview values. When every seat has confirmed, enter `readyToStart`. |
| `readyToStart` | The paired board or host selects **Start game** for round one or **Start round** for later rounds. | Enter `playing` and set the first turn in the same mutation. |
| `playing` | A legal move ends the round under the Cabo or deck exhaustion rules. | Score once and enter `roundResults` or `gameOver`. |

The board displays readiness by seat. It shows its start button only when the server includes `beginPlay` in the board action list. The host phone retains the same start action so a missing or disconnected board cannot block play. The board never sees the opening card values or generates player confirmations. **I've looked** is an explicit player acknowledgment. The server can verify that a reveal occurred, and the player confirms that they saw it.

Keep the frozen roster and each seat's preview status in server state. Connection changes neither confirm a seat nor remove it from readiness. If a seat cannot return during the first-round preview, the host can cancel setup and return to the lobby. After a later deal, cancel back to the last results without changing completed scores. Discard the abandoned deal and readiness in both cases. In the first playable version, a confirmed seat remains ready until that setup is canceled or play begins.

### Transition and retry invariants

- Use an explicit phase and per-seat preview state. Derive action descriptors and validate commands from the same guards. No turn actions during setup, no player joins after dealing, and no start until every frozen seat confirms.
- Serialize mutations per room. Authenticate the current credential, check command replay, then validate the revision, phase, role, and targets. Two simultaneous starts cause one transition. A stale request returns the latest role-scoped snapshot so the client can refresh and resubmit intentionally.
- Scope setup to a unique deal instance. A delayed reveal, confirmation, start, or cancel from an abandoned deal cannot mutate a replacement. A retry of a committed deal returns the result of that deal without drawing cards again.
- During play, model each turn as awaiting a draw or Cabo call, resolving a draw, or resolving a power. Validate actor, action, and targets before committing. A resolved move advances at most one turn. Keep Cabo's remaining seats explicit and score at most once per round, including if deck exhaustion and the last final turn coincide.
- Cover early start, confirmation before reveal, duplicate or foreign slot references, attempts to change the revealed pair, reconnect before and after confirmation, duplicate requests, overlapping start from two devices, stale commands after cancel, and board credential misuse. In gameplay tests, exercise wrong-seat moves and one-time scoring after Cabo and deck exhaustion.

## Client/server boundary

The wire contract is versioned JSON. `shared/` will define the Rust types for snapshots, action descriptors, and commands. A native client can implement the same contract directly in Swift. Bevy and native clients own rendering, accessibility, animation, selection, and transport. Game outcomes, visibility, phase transitions, scoring, and available actions come from the server. The server uses shared rules to validate moves and enumerate actions from the same revision.

### Full snapshot for each seat

Create, join, load or resume, poll, and authenticated command responses return a complete snapshot for the requesting seat. Each snapshot can render the current screen by itself. Use the same shape if push updates are added later.

| Snapshot field | Contents |
| --- | --- |
| `protocolVersion`, `roomId`, `revision` | Wire compatibility, room instance identity, and a monotonically increasing room revision |
| `roomCode`, `youSeatId`, `hostSeatId` | Invite code, requesting seat, and host |
| `phase`, `round`, `turnSeatId` | Lobby, initial preview, ready to start, active round, round results, or game over, plus the current turn when applicable |
| `setup` | Deal identifier and per-seat readiness during setup. Only the requesting seat sees its own preview values while revealed. |
| `players`, `table` | Ordered seats, scores, card slots and visible values, deck count, and discard top |
| `pendingMove`, `finalTurns`, `results` | Player-visible draw or power state, Cabo caller and remaining turns, and round or game results as applicable |
| `availableActions` | Actions this seat can submit at this revision, including the choices each action accepts |

Project the authoritative state into a distinct per-seat response type. Include hidden card slots with opaque references so clients can render card backs and select targets where allowed. Reveal only public values and values the requesting seat is entitled to see. Keep deck order, private card values, and seat credentials out of the snapshot. Action descriptors must also preserve hidden information. Send private snapshots with `Cache-Control: no-store`.

Fetch on load or reconnect before enabling gameplay controls, then poll. Apply a snapshot as a replacement for the active room and seat. Ignore responses from previous sessions and snapshots older than the current revision. An equal revision can complete a refresh without resetting local selection. When the revision advances, retain a selection only if the new descriptor still permits it. Keep local pending feedback separate from game state.

### Action catalog

Use stable, tagged action kinds and typed parameters. Each descriptor carries any applicable choice sets, target references, count constraints, and required choices. Use the action kind plus the selected parameters to submit a command. The catalog describes game intent independently of button labels, layout, and input device.

| Action kind | Choices supplied by the server |
| --- | --- |
| `dealRound`, `dealNextRound`, `dealNewGame`, `cancelSetup` | Host only, in the applicable phase. Cancellation identifies the current deal. |
| `revealInitialCards` | Current deal, with exactly two distinct slot references from the server-provided list of the requesting seat's four starting slots. |
| `confirmInitialLook` | Current deal. The requesting seat must already have revealed a pair. |
| `beginPlay` | Current deal. Available to the paired board and host only after all seats confirm. |
| `draw` | Currently available piles, such as `deck` or `discard`. |
| `discardDrawn`, `useDrawnPower`, `callCabo` | No parameters. For a power, include the power kind visible to the acting player. |
| `exchange` | Selectable own card slots, minimum and maximum selection counts, and valid insertion sides for a failed exchange when applicable. |
| `peek` | Selectable own card slots. |
| `spy` | Selectable opponent card slots grouped by seat. |
| `swap` | Selectable own slots and opponent slots, with explicit pair constraints if needed. |

Create room, join room, and resume session are entry flows outside the in-room catalog. A seat with no available commands receives an empty array. For an exchange, list selectable slots and count constraints without enumerating matching sets. The server resolves the outcome using hidden card values. Clients can choose their own presentation for waiting and unsupported actions. Unknown action kinds must never be submitted by an older client.

### Command and response flow

1. Submit a unique `commandId`, the snapshot's `expectedRevision`, an action kind, and its selected parameters. Authenticate with the appropriate private seat or board credential. The public room code locates the room.
2. Within one serialized room update, authenticate the session, check command replay, check the expected revision, then validate the command and parameters against current state. Commit the entire accepted mutation and advance the revision once.
3. Return the command outcome alongside a fresh snapshot projected for that session's role. Authenticated stale or illegal commands return a machine-readable rejection and the latest snapshot without changing state. A retry with the same command ID and payload returns the original outcome and a fresh snapshot without replaying the mutation. Reusing an ID with a different payload is rejected.

### Frontend parity

Keep canonical JSON fixtures for lobby, waiting, draw, each card power, round results, and game over when implementing the contract. Include asymmetric visibility fixtures for two seats at the same revision. Each frontend should verify that the fixture produces the same semantic controls and that selections encode the same command payloads. Also exercise direct load into an active game, an empty action list, an unknown action kind, and out-of-order snapshots. Native and browser frontends can use different layouts and gestures.

## Tabletop mode

Tabletop mode is an optional way to play the same room. An iPad lying flat shows the shared table while each player uses a phone in landscape for their private hand. Players can use the regular client view alongside tabletop devices in the same game.

### Pair a board

1. The host opens **Pair a board** on their player device and gets a short-lived, single-use pairing code scoped to the room.
2. On the iPad, choose **Join as board** and enter the pairing code. The server exchanges it for a board session credential with permission to fetch public snapshots and start a ready round. The board can attach during the lobby or an active game.
3. The iPad resumes with its board credential and fetches the latest full board snapshot on load or reconnect. The host can revoke the pairing from their phone. A revoked or expired session returns the iPad to pairing and clears its displayed room state.

The board role uses no player seat and does not count toward the two-to-four-player limit. Pairing and reconnecting leave the turn order and game revision intact. The room can continue if the iPad disconnects. The invite code remains the player join flow.

### What each device shows

| Device | View and controls |
| --- | --- |
| iPad board | Show a large deck with its remaining count, the face-up discard, player names and seat positions, current turn, Cabo final turns, scores, and public round or game results. Make the current player readable from around the table. Show readiness during setup, the start button when offered by the server, and a reconnecting indicator if refresh fails. |
| Player phone | Show the player's four starting slots in a landscape row. Render the actual hand size as exchanges and penalties change it. Show card faces only when the seat snapshot allows them, including private draws, Peek, or Spy. Render the server's available actions and selectable targets, and show enough public context to finish a turn without the iPad. |
| Host phone | Inherits the player view and provides board pairing and revocation in addition to phase-appropriate host controls. |

### Landscape phone design

Give the private hand most of the screen. Use a compact status strip for phase, turn, and connection, a centered row of cards, and a bottom action strip inside the device safe area. Use a dark green table, warm card faces, and large rank numbers. Use an outline and a checkmark for selection so it reads without color. Keep labels and tap targets large enough to use with either thumb.

```text
 Opening look                                     Choose 2 cards

     [ 1 ▧ ]       [ 2 ✓ ]       [ 3 ▧ ]       [ 4 ✓ ]

 2 of 2 selected                                  [ Reveal 2 ]
```

- **Opening selection.** Start with four face-down cards in fixed positions. Tap a card to select or deselect it. Enable **Reveal 2** when two are selected. Tapping a third card prompts the player to deselect one. Submit the pair together and turn over only the two cards returned by the server.
- **Private reveal.** Keep the other two cards face down. Replace **Reveal 2** with **I've looked**. Confirmation turns both preview cards face down and shows **Waiting for players** or **Ready to start**, as dictated by the server. Keep the card positions stable through the transition.
- **Active turn.** Keep the hand central. Use the action strip for the controls offered by the current snapshot, with the deck and discard available when relevant. When selecting a target for Spy or Swap, show eligible opponent slots in a temporary target row. Group a larger hand into a scrollable row if needed without shrinking tap targets.
- **Without an iPad.** Keep a compact public table strip with the turn, deck count, and discard top. In tabletop mode that context can take less space. Rotation or a network refresh preserves choices still supported by the latest snapshot. A page reload after a reveal restores the same selected pair from server state.

### Snapshot and permission boundary

Use the same room and revision stream for both roles. Produce a complete public `BoardSnapshot` from the authoritative state on the server. Reuse public wire types from `shared/` and keep private seat fields out of the board response type. The board receives no private hand knowledge, private draw values or power results, seat credentials, or player action descriptors. Card values revealed publicly at scoring may appear on the board. The board credential authorizes public board reads and the single `beginPlay` command. Authorize that command only for an active pairing in this room and a ready deal. Require a player session for every other game command. Board command responses use the public projection.

Phones still receive their normal complete seat snapshots. Landscape layout is a presentation choice. Any Bevy or native client can render the board or a player hand from the relevant role's snapshot.

### Acceptance

- Pair an iPad and join on two phones. Both phones and the board can refresh into the same current round without depending on previously received updates.
- At the same revision, the board shows the public turn and piles. Only the authorized phone sees its private draw or power result. A board credential cannot read a seat view or submit player moves. Verify that its start button appears only after all phones confirm and that tapping it starts exactly one turn.
- Rotate a phone to landscape, carry out a turn, then reload mid-round. The hand and available controls are restored from the latest seat snapshot.
- Disconnect or revoke the board while the phones continue playing. Reconnection restores public state with a valid board session. Revocation requires a fresh pairing.

## Hosting

Use one Railway service on `PORT` for the compiled Bevy client and API. Keep `GET /health` independent of game traffic. For the first playable version, scope in-memory rooms to one replica, cap room creation, expire inactive rooms, and disclose that restarts end active games. Durable games and multiple replicas are later work.

## Implementation milestones

- **Scaffold** — Bevy demo, Axum static hosting and health route, and a shared crate with draw validation.
- **Lobby** — Create, join, resume, and deal rooms. Wire the opening preview and readiness gate. Acceptance requires two independent browser sessions to see the same lobby while keeping seat credentials private.
- **Playable round** — Port turns, card actions, and scoring. Acceptance requires a full two-player round, rejected illegal or stale moves, and verified player-specific redaction.
- **Full game** — Port round progression and end-game behavior. Acceptance requires reconnect during play, safe command retry, and a complete game from invite to final scores.
- **Tabletop mode** — Add board pairing, the public board snapshot, the board start button, and the landscape hand view. Verify the same room from an iPad and two independent phone sessions.
