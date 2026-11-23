# Cabo Club product and engineering spec

## Goal

Two to four players can open a browser, create a private table or join via an invite, and play a full Cabo game. The host starts the game and opens subsequent rounds. Players keep their seat when reconnecting with a valid session.

## Ownership

- **app/** renders the lobby, table, scores, and results with Bevy. It translates input into commands, renders player-specific snapshots, and uses shared validation to guide available controls.
- **server/** serves the browser build, creates and expires rooms, issues seat credentials, authenticates players, shuffles and deals, advances games, and publishes views. It serializes updates within each room and owns the authoritative game state.
- **shared/** owns pure rule types, move validation, and scoring. Transport-neutral request and view types belong here when introduced. Full state and player-visible snapshots require distinct types so hidden information cannot accidentally be serialized into client responses.

## Game lifecycle

1. Create a room, return a shareable code and a separate private seat credential, then allow up to four seats while the lobby is open.
2. The host starts with at least two players. Deal four hidden cards per player and privately reveal the first two to their owner. Show the discard pile's top card and the number of cards left in the deck.
3. On a player's turn, allow a legal draw, discard, exchange, special ability, or Cabo call. Apply exchanges and their penalties atomically. Deck cards 7–8 allow Peek, 9–10 allow Spy, and 11–12 allow Swap. A Cabo call grants every other player one final turn. Deck exhaustion ends the round.
4. Reveal hands at round end. A caller tied for the lowest hand scores zero. Otherwise the caller adds ten to their hand total. A hand of 12, 12, 13, 13 scores zero and awards each opponent 50. Exactly 100 resets a player's total to 50 once per game. A total above 100 ends the game and the lowest total wins, with last-round points breaking ties where possible.
5. The host starts the next round or a new game after completion. Preserve the deployed app's detailed match, penalty, turn order, and scoring behavior in regression cases during the port.

## Multiplayer contract

Start with same-origin JSON endpoints for create, join, resume or refresh, and submit command. Use snapshot polling for the first playable version. The interface can add push updates later without moving game authority to the client.

- Require a private seat credential for subsequent requests. The invite code locates a room and can be shared. Keep credentials out of invite URLs, room snapshots, and logs.
- A command carries a unique command ID and the room revision the client acted on. Stale commands return a fresh authorized snapshot. Repeating an accepted ID for the same seat returns its recorded outcome without replaying the move.
- The server checks authentication, host privileges, phase, turn ownership, and shared rules before committing. Each accepted mutation advances the room revision. Rejections leave game state unchanged.
- The response is built for the requesting seat. Before scoring, reveal only public cards and values that seat is entitled to see. Include turn, phase, scores, and revision. Keep deck order, credentials, and other private card values out of responses.

## Hosting

Use one Railway service on `PORT` for the compiled Bevy client and API. Keep `GET /health` independent of game traffic. For the first playable version, scope in-memory rooms to one replica, cap room creation, expire inactive rooms, and disclose that restarts end active games. Durable games and multiple replicas are later work.

## Implementation milestones

- **Scaffold** — Bevy demo, Axum static hosting and health route, and a shared crate with draw validation.
- **Lobby** — Create, join, resume, and start rooms. Acceptance requires two independent browser sessions to see the same lobby while keeping seat credentials private.
- **Playable round** — Port turns, card actions, and scoring. Acceptance requires a full two-player round, rejected illegal or stale moves, and verified player-specific redaction.
- **Full game** — Port round progression and end-game behavior. Acceptance requires reconnect during play, safe command retry, and a complete game from invite to final scores.
