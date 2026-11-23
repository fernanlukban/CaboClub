# Cabo Club product and engineering spec

## Goal

Two to four players can open a browser, create a private table or join via an invite, and play a full Cabo game. The host or a room-creating board deals rounds. The iPad board begins play once every player confirms their opening look. The host can start when no board session is authorized. Players keep their seat when reconnecting with a valid session.

## Ownership

- **app/** renders the lobby, table, scores, and results with Bevy. It renders player-specific snapshots, presents the server's enumerated actions, and translates selections into commands.
- **server/** serves the browser build, creates and expires rooms, issues seat credentials, authenticates players, shuffles and deals, advances games, and publishes views. It serializes updates within each room and owns the authoritative game state.
- **shared/** owns pure rule types, move validation, and scoring. Transport-neutral request, snapshot, and action descriptor types belong here when introduced. Full state and player-visible snapshots require distinct types so hidden information cannot accidentally be serialized into client responses.

## Game lifecycle

1. Create a room, return a shareable code and a separate private seat credential, then allow up to four seats while the lobby is open.
2. Deal four hidden cards per player with at least two players present. Each player privately chooses two of their four cards to look at and confirms before the board begins play. Show the discard pile's top card and the number of cards left in the deck.
3. On a player's turn, allow a legal draw, discard, exchange, special ability, or Cabo call. Apply exchanges and their penalties atomically. Deck cards 7–8 allow Peek, 9–10 allow Spy, and 11–12 allow Swap. A Cabo call grants every other player one final turn. Deck exhaustion ends the round.
4. Reveal hands at round end. A caller tied for the lowest hand scores zero. Otherwise the caller adds ten to their hand total. A hand of 12, 12, 13, 13 scores zero and awards each opponent 50. Exactly 100 resets a player's total to 50 once per game. A total above 100 ends the game and the lowest total wins, with last-round points breaking ties where possible.
5. The host or room-creating board deals the next round or a new game after completion. Preserve the deployed app's detailed match, penalty, turn order, and scoring behavior in regression cases during the port.

## Round-start state machine

Dealing and starting play happen in two separate steps. Use the same flow for the first and subsequent rounds, with or without a board.

| State | Event and guard | Result |
| --- | --- | --- |
| `lobby`, `roundResults`, or `gameOver` | The host or room-creating board selects **Deal cards**, **Deal next round**, or **New game**, respectively. Require two to four seats. | Freeze the round roster, deal once, reset readiness, and enter `initialPreview`. There is no active turn yet. |
| `initialPreview` | A seated player chooses two distinct slots from their own four starting cards and taps **Reveal 2**. | Record the selected pair for this deal and reveal those two values in the seat's private snapshot. A reconnect before confirmation restores the same pair. Further reveal requests cannot change it. |
| `initialPreview` | The same player selects **I've looked** after revealing. | Record readiness and hide that seat's opening preview values. Remove the two ranks from subsequent seat snapshots, replay responses, and remembered-card UI. A later game-authorized reveal or public scoring can show them again. When every seat has confirmed, enter `readyToStart`. |
| `readyToStart` | The authorized board, or the host when no board is authorized, selects **Start game** for round one or **Start round** for later rounds. | Enter `playing` and set the first turn in the same mutation. |
| `playing` | A legal move ends the round under the Cabo or deck exhaustion rules. | Score once and enter `roundResults` or `gameOver`. |

The board displays readiness by seat. It shows its start button only when the server includes `beginPlay` in the board action list. If the board disconnects before starting, reconnect it, pair another board, or revoke board access and start from the host phone. The board never sees the opening card values or generates player confirmations. **I've looked** is an explicit player acknowledgment. The server can verify that a reveal occurred, and the player confirms that they saw it.

Keep the frozen roster and each seat's preview status in server state. Connection changes neither confirm a seat nor remove it from readiness. If a seat cannot return during the first-round preview, the host can cancel setup and return to the lobby. After a later deal, cancel back to the last results without changing completed scores. Discard the abandoned deal and readiness in both cases. In the first playable version, a confirmed seat remains ready until that setup is canceled or play begins.

### Transition and retry invariants

- Use an explicit phase and per-seat preview state. Derive action descriptors and validate commands from the same guards. No turn actions during setup, no player joins after dealing, and no start until every frozen seat confirms.
- Serialize mutations per room. Authenticate the current credential, check command replay, then validate the revision, phase, role, and targets. Two simultaneous starts cause one transition. A stale request returns the latest role-scoped snapshot so the client can refresh and resubmit intentionally.
- Scope setup to a unique deal instance. A delayed reveal, confirmation, start, or cancel from an abandoned deal cannot mutate a replacement. A retry of a committed deal returns the result of that deal without drawing cards again.
- During play, model each turn as awaiting a draw or Cabo call, resolving a draw, or resolving a power. Validate actor, action, and targets before committing. A resolved move advances at most one turn. Keep Cabo's remaining seats explicit and score at most once per round, including if deck exhaustion and the last final turn coincide.
- Cover early start, confirmation before reveal, duplicate or foreign slot references, attempts to change the revealed pair, reconnect before and after confirmation, duplicate requests, overlapping start from two devices, stale commands after cancel, and board credential misuse. In gameplay tests, exercise wrong-seat moves and one-time scoring after Cabo and deck exhaustion.

## Turn flow and matching

| Turn state | Actions and result |
| --- | --- |
| Awaiting a move | Pull from the face-down deck or tap the public discard top. The server may also offer **Call match** with selected hand slots if there is a discard top to take, or **Call Cabo** when allowed. |
| Resolving a discard draw | Move the card taken from the discard pile into a temporary slot at the edge of the player's hand. Show the incoming rank and let the player select an existing hand card to discard, or select multiple existing cards and call match. |
| Resolving a deck draw | Show the drawn card privately beside the hand. The player can keep it by trading it for a hand card, discard it, use its power if offered, or select multiple hand cards and call match. |
| Resolving a power | Show only the targets and actions the server offers for Peek, Spy, or Swap. |
| Move complete | Apply the card movement, hide retained private cards, and advance the turn once. |

Players call a match on two or more distinct cards from their own hand. The server compares the selected cards with each other. The incoming card can have any rank. A player who draws first can compare its value with the expected total of the cards they want to replace. The client displays the incoming rank and leaves any arithmetic about hidden cards to the player.

- **Match first.** Capture the current discard top as the incoming card before placing any matched cards on the pile. On success, put the selected cards on the discard pile and keep the captured card at the first selected slot. Taking the discard and resolving the match is one server mutation. A player cannot take one of the cards they just matched.
- **Draw first.** Use the already drawn card as the replacement. On success, put all selected cards on the discard pile and keep the incoming card at the first selected slot. A successful match of three cards shrinks the hand by two slots. Preserve the relative order of the other cards and keep the discard order deterministic.
- **Failed match.** Resolve the existing failed-exchange penalty, incoming-card placement, and next turn as one server mutation. Match eligibility describes which slots are selectable and how many can be selected. Matching ranks are never hinted at by the server's action descriptors.

Send the selected slot references with `callMatch`. Derive the incoming source from the authoritative turn state. Validate the complete command before touching the deck, discard, or hand. Replaying an accepted command must neither repeat the move nor create another penalty.

## Client/server boundary

The wire contract is versioned JSON. `shared/` will define the Rust types for snapshots, action descriptors, and commands. A native client can implement the same contract directly in Swift. Bevy and native clients own rendering, accessibility, animation, selection, and transport. Game outcomes, visibility, phase transitions, scoring, and available actions come from the server. The server uses shared rules to validate moves and enumerate actions from the same revision.

### Full snapshot for each seat

Create, join, load or resume, poll, and authenticated command responses return a complete snapshot for the requesting seat. Each snapshot can render the current screen by itself. Use the same shape if push updates are added later.

| Snapshot field | Contents |
| --- | --- |
| `protocolVersion`, `roomId`, `revision` | Wire compatibility, room instance identity, and a monotonically increasing room revision |
| `roomCode`, `youSeatId`, `hostSeatId` | Invite code, requesting seat, and host |
| `phase`, `round`, `turnSeatId` | Lobby, initial preview, ready to start, active round, round results, or game over, plus the current turn when applicable |
| `dealId` and player `ready` | Deal identifier and per-seat readiness during setup. Only the requesting seat sees its own preview values while revealed. |
| `players`, `deckCount`, `discardTop` | Ordered seats, scores, card slots and visible values, deck count, and discard top |
| `incoming`, `finalTurns`, `results` | Player-visible draw or power state, Cabo caller and remaining turns, and round or game results as applicable |
| `availableActions` | Actions this seat can submit at this revision, including the choices each action accepts |

Project the authoritative state into a distinct per-seat response type. Include hidden card slots with opaque references so clients can render card backs and select targets where allowed. Reveal only public values and values the requesting seat is entitled to see. Keep deck order, private card values, and seat credentials out of the snapshot. Action descriptors must also preserve hidden information. Send private snapshots with `Cache-Control: no-store`.

Fetch on load or reconnect before enabling gameplay controls, then poll. Apply a snapshot as a replacement for the active room and seat. Ignore responses from previous sessions and snapshots older than the current revision. An equal revision can complete a refresh without resetting local selection. When the revision advances, retain a selection only if the new descriptor still permits it. Keep local pending feedback separate from game state.

### Action catalog

Use stable, tagged action kinds and typed parameters. Each descriptor carries any applicable choice sets, target references, count constraints, and required choices. Use the action kind plus the selected parameters to submit a command. The catalog describes game intent independently of button labels, layout, and input device.

| Action kind | Choices supplied by the server |
| --- | --- |
| `dealRound`, `dealNextRound`, `dealNewGame` | Host or room-creating board, in the applicable phase. |
| `cancelSetup` | Host only, in setup. Identifies the current deal. |
| `revealInitialCards` | Current deal, with exactly two distinct slot references from the server-provided list of the requesting seat's four starting slots. |
| `confirmInitialLook` | Current deal. The requesting seat must already have revealed a pair. |
| `beginPlay` | Current deal. Available after all seats confirm to the authorized board or to the host if no board is authorized. |
| `drawDeck`, `drawDiscard` | No parameters. Each appears when the corresponding pile can be taken. |
| `discardDrawn`, `useDrawnPower`, `callCabo` | No parameters. For a power, include the power kind visible to the acting player. |
| `discardHandCard` | One selectable existing hand slot to trade for the current incoming card. |
| `callMatch` | Selectable own slots, minimum of two, maximum set by the current hand and game rules, and any required placement choice for a failed match. Offer before a draw if the discard top is available, or while resolving an incoming card. |
| `peek` | Selectable own card slots. Enters an actor-only private view. |
| `acknowledgePeek` | Current deal and power instance. Clears the private view and completes the turn. |
| `skipTurn` | Host only during play. Current deal and revision. Settles any pending draw to discard, clears any unfinished power, then completes the active turn normally. |
| Spy and Swap | See [the tabletop power contract](TABLETOP_FLOWS.md#wire-contract-changes). |

Create room, join room, and resume session are entry flows outside the in-room catalog. A seat with no available commands receives an empty array. The actor can receive a rank-free `matchOutcome` at the accepted revision with source, placement, discarded slots, and an optional penalty slot. This field is omitted for other roles and at later revisions. For a match, list selectable slots and count constraints without enumerating matching sets. The server resolves the outcome using hidden card values. Clients can choose their own presentation for waiting and unsupported actions. Unknown action kinds must never be submitted by an older client.

### HTTP endpoints

| Request | Result |
| --- | --- |
| `POST /api/rooms` | Create with `{ "board": true }` or with `board: false` and a player `name`. Returns a credential and snapshot. |
| `POST /api/rooms/{code}/join` | Join with a `name`. Returns a seat credential and snapshot. |
| `POST /api/rooms/{code}/board` | Redeem a pairing `code`. Returns a board credential and snapshot. |
| `GET /api/rooms/{code}` | Fetch a complete snapshot for the bearer credential. |
| `POST /api/rooms/{code}/commands` | Send a command with the bearer credential. Returns `outcome`, `detail`, and the current snapshot. |

### Command and response flow

1. Submit a unique `commandId`, the snapshot's `expectedRevision`, an action kind, and its selected parameters. Authenticate with the appropriate private seat or board credential. The public room code locates the room.
2. Within one serialized room update, authenticate the session, check command replay, check the expected revision, then validate the command and parameters against current state. Commit the entire accepted mutation and advance the revision once.
3. Return the command outcome alongside a fresh snapshot projected for that session's role. Authenticated stale or illegal commands return a machine-readable rejection and the latest snapshot without changing state. A retry with the same command ID and payload returns the original outcome status and a fresh snapshot. Keep private card values in the current authorized snapshot only. The mutation is never replayed. Reusing an ID with a different payload is rejected.

### Frontend parity

Keep canonical JSON fixtures for lobby, waiting, draw, each card power, round results, and game over when implementing the contract. Include asymmetric visibility fixtures for two seats at the same revision. Each frontend should verify that the fixture produces the same semantic controls and that selections encode the same command payloads. Also exercise direct load into an active game, an empty action list, an unknown action kind, and out-of-order snapshots. Native and browser frontends can use different layouts and gestures.

## Tabletop mode

See [Tabletop powers and TEST room](TABLETOP_FLOWS.md) for the Spy and Swap interactions and bot testing flow.

Tabletop mode is an optional way to play the same room. An iPad lying flat shows the shared table while each player uses a phone in landscape for their private hand. Players can use the regular client view alongside tabletop devices in the same game.

### Open tabletop mode on the iPad

The dedicated `/tabletop` route stays in board mode across refreshes and reconnects. The tabletop home screen offers **Join a game** and **Create a new game**.

- **Join a game.** Resume an existing board session if available. Otherwise enter a short-lived board pairing code from the host. Load the latest complete board snapshot and open directly into the room's current phase, including a game already in progress.
- **Create a new game.** Create the lobby and its board session together. Immediately show a QR code containing the player join URL and a text room code for manual entry. Phones scanning the QR open the Bevy web app at the room's join screen. After choosing a name, each phone receives its own private seat credential and opens the landscape hand view. Keep board and seat credentials out of the QR.
- **Deal and start.** A board that created the room can deal, then show readiness and offer **Start game** once every player confirms. The first phone to join becomes the player host and can issue pairing codes and cancel a deal. A board that joins an existing room can start a ready deal. That room's host retains deal and cancel controls.

The board uses no player seat. Pairing and reconnecting preserve the roster and turn order. If the iPad disconnects, phones can continue. If the board credential expires or is revoked, clear the displayed room and return to tabletop home.

### What each device shows

| Device | View and controls |
| --- | --- |
| iPad board | Show four fixed cardinal seat positions for public card states, a central deck with its remaining count, the face-up discard next to it, current turn, Cabo final turns, scores, and public round or game results. Make the current player readable from around the table. Show readiness during setup, the start button when offered by the server, and a reconnecting indicator if refresh fails. |
| Player phone | Show the player's four starting slots in a landscape row. Render the actual hand size as exchanges and penalties change it. Show card faces only when the seat snapshot allows them, including private draws, Peek, or Spy. Render the server's available actions and selectable targets, and show enough public context to finish a turn without the iPad. |
| Host phone | Inherits the player view and provides board pairing and revocation in addition to phase-appropriate host controls. |

### Landscape phone design

Render cards as 2D Bevy sprites with soft shadows, slight perspective and tilt, and a slow floating idle motion. Keep hit areas steady while cards float. Use a dark green table, readable warm card faces, and a selection outline and checkmark.

| View | Composition |
| --- | --- |
| Opening look | Four stationary card positions. Tap any two card backs and select **Reveal 2**. After **I've looked**, flip both cards face down and show readiness. |
| Waiting | Make the player's hand the only card row and keep the phase and turn in a small status strip. Hide turn controls. |
| Your turn | Ease the camera out and translate the hand down as the cards scale smaller. Bring a second row into view above the hand, with the face-down deck and face-up discard top centered. Show buttons and touch targets for the current server actions. |
| Resolving a draw or match | Animate the incoming card from the source pile into the hand area. Raise selected cards. Animate a confirmed match or discard toward the discard pile, then reflow the hand. |
| Turn ends | Ease back to the private hand view. |

Fit the entire hand inside the landscape viewport without scrolling. Recompute spacing and scale for the actual hand size, including a pending incoming card. When needed, tighten spacing or use staggered rows with each card independently visible and tappable. Preserve card positions by stable slot reference across revisions. Keep controls inside the device safe area.

Use ease-in-out interpolation for camera zoom, camera translation, and card movement. Tapping the discard top or dragging from the deck sends the same command as the equivalent button. Enable gestures and buttons from the latest action descriptors. The animation presents confirmed state. A new snapshot can interrupt it and settle into the latest view. Offer reduced motion with the same controls and immediate layout changes.

### Snapshot and permission boundary

Use the same room and revision stream for both roles. Project a complete public board snapshot from the authoritative state on the server. The shared JSON envelope identifies the board role and has null values for seat-specific fields. The board receives no private hand knowledge, private draw values or power results, seat credentials, or player action descriptors. Card values revealed publicly at scoring may appear on the board. The board credential authorizes public board reads, `beginPlay` for a ready deal, and target staging for a pending Spy or Swap. Authorize deal commands only for the board session that created the room. Bind every board command to the current room, phase, and session. Test rooms also expose restart controls to their creating board. Require a player session for every other game command. Board command responses use the public projection.

Keep the board route explicit when persisting the room URL and use distinct board and player sessions. The board projects card backs until ranks are public and uses the same seat position throughout the game. Phones still receive their normal complete seat snapshots. Landscape layout is a presentation choice. Any Bevy or native client can render the board or a player hand from the relevant role's snapshot.

### Acceptance

- Create a room on the iPad and scan its QR on two phones. Both phones and the board can refresh into the same current round without depending on previously received updates.
- At the same revision, the board shows the public turn and piles. Only the authorized phone sees its private draw or power result. A board credential cannot read a seat view or submit player moves. Verify that its start button appears only after all phones confirm and that tapping it starts exactly one turn.
- Join the iPad to an in-progress room and verify it opens at the current phase. Confirm the opening look on a phone and reload. Both cards stay face down.
- Verify a landscape phone fits the entire hand without scrolling before and after a two-card match, a three-card match, and a penalty. Exercise discard pickup, matching before a draw, and matching after a draw. Reload mid-turn to restore the pending card and available actions.
- Disconnect or revoke the board while the phones continue playing. Reconnection restores public state with a valid board session. Revocation requires a fresh pairing.

## Hosting

Use one Railway service on `PORT` for the compiled Bevy client and API. Keep `GET /health` independent of game traffic. For the first playable version, scope in-memory rooms to one replica, cap room creation, expire inactive rooms, and disclose that restarts end active games. Durable games and multiple replicas are later work.

## Implementation milestones

- **Scaffold** — Bevy demo, Axum static hosting and health route, and a shared crate with draw validation.
- **Lobby** — Create, join, resume, and deal rooms. Wire the opening preview and readiness gate. Acceptance requires two independent browser sessions to see the same lobby while keeping seat credentials private.
- **Playable round** — Port turns, card actions, and scoring. Acceptance requires a full two-player round, rejected illegal or stale moves, and verified player-specific redaction.
- **Full game** — Port round progression and end-game behavior. Acceptance requires reconnect during play, safe command retry, and a complete game from invite to final scores.
- **Tabletop mode** — Add board pairing, the public board snapshot, the board start button, and the landscape hand view. Verify the same room from an iPad and two independent phone sessions.
