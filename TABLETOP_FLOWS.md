# Tabletop powers and TEST room

Product and protocol spec for [Cabo Club](SPEC.md). Spy and Swap use shared targeting, and staging includes a TEST room with bots.

## Device flows

| Step | Initiating phone | Shared iPad | Other phones |
| --- | --- | --- | --- |
| Activate | Tap **Use Spy** or **Use Swap** after a qualifying draw. | Show which player is choosing. | Continue to show the private hand and current turn. |
| Select own card for Swap | Tap one own card. Show **Choose a card to give** until selected. | Show **Waiting for [name] to choose their card**. | Keep normal waiting view. |
| Select opponent card | Show **Choose an opponent card** and highlight eligible slots. The player can tap the same compass seat on the phone’s mini table or use the iPad. | Enable only eligible opponent card backs. Tapping one highlights the same slot on both devices. | Keep normal waiting view. |
| Review | Show **Confirm Spy** or **Confirm Swap**. Allow a new target or **Change my card** for Swap. | Show **Waiting for [name] to confirm**. | Keep normal waiting view. |
| Spy result | Show a private toast with the rank, for example **Spy confirmed · Bea’s card is 7**. Tap **Done** to hide it and pass the turn. | Show **[Name] is finishing their turn**. Clear target highlights when the reveal begins. | See waiting state only. |
| Swap result | Show **Swap complete** as a private toast. Animate the two cards trading places without showing ranks. | Animate the public card backs moving and highlight the next turn. | The affected player sees their hand update without revealing either rank. |

An iPad tap stages a target. The initiating phone confirms it. This lets a player use either screen while keeping a shared-screen mis-tap recoverable. No additional confirmation is necessary after **Confirm Swap**. The **Done** button in Spy closes a private look. It is not a second confirmation of the target.

## Server states and transitions

A power instance belongs to a room, deal, and acting seat and has a unique `powerId`. Model the following as distinct turn variants. Fields in a variant exist only when they are valid for that step.

| State | Accepted event and role | Atomic result |
| --- | --- | --- |
| `incoming` | Actor uses the drawn Spy or Swap card | Discard that draw once and create `spySelecting` or `swapSelectingOwn`. |
| `spySelecting` | Actor or authorized board stages an eligible opponent slot | Enter `spyReview` with that slot. |
| `spyReview` | Actor or authorized board stages another eligible opponent slot | Replace the staged slot. |
| `spyReview` | Actor confirms the staged slot | Enter `spyViewing` and deliver the rank only to that actor. Keep the turn locked. |
| `spyViewing` | Actor acknowledges the look | Clear the private result and advance the turn or score the round once. |
| `swapSelectingOwn` | Actor chooses an own slot | Enter `swapSelectingOpponent` with the own slot. |
| `swapSelectingOpponent` or `swapReview` | Actor changes their own slot | Replace the own slot and clear any staged opponent target. |
| `swapSelectingOpponent` or `swapReview` | Actor or authorized board stages an eligible opponent slot | Enter `swapReview` with both slots. |
| `swapReview` | Actor confirms both staged slots | Swap both cards and advance the turn or score the round in one mutation. Record a private success receipt. |

Only the paired, unrevoked board can submit a board target. Its permission comes from the current descriptor. It ends as soon as the state leaves selection or review. The board can never choose an own slot, initiate a power, confirm a target, acknowledge a private look, or take a player turn. The actor can complete the flow on the phone if the board disconnects.

In each room mutation, authenticate, check command replay, then validate the deal, revision, power instance, phase, actor, and slot ownership. Commit the complete transition under the same room lock. For a race between a phone and iPad tap, one wins at the submitted revision. The other gets the latest snapshot and can retry a selection intentionally. Confirm commands echo the staged slot references as a guard against a last-moment target change. Invalid or stale requests never move cards or reveal ranks.

Reconnect renders the current phase directly. A player who reconnects during `spyViewing` gets the same private result until **Done**. After acknowledgment, neither snapshots nor replay outcomes include the rank. The original replay outcome records command status, not old private views.

## Wire contract changes

Keep complete snapshots for both roles. [Wire fixtures](shared/fixtures/tabletop-powers.json) show the power-related fields for each device; snapshots also include the ordinary room state. Add the following fields and descriptors as an additive change to the current protocol. Existing clients can ignore unknown actions.

| Contract | Shape and visibility |
| --- | --- |
| `pendingPower` | `powerId`, kind, actor seat, and phase. Project only what each device needs. The actor gets their selected own slot and staged target. The board gets staged target and eligible opponent slots through its descriptor. Other seats see public progress only. |
| `privatePowerResult` | Actor only. Contains result ID, kind, and the Spy rank until acknowledged. Swap includes its success receipt without ranks. Use the result ID to deduplicate toasts after polling or a command retry. |
| `selectPowerOwnCard` | Actor only. Pick one own slot. |
| `stagePowerTarget` | Actor or scoped board during an eligible selection state. Pick one opponent slot from `availableActions`. |
| `confirmSpy`, `confirmSwap` | Actor only. Echo the exact staged selection. |
| `acknowledgeSpy` | Actor only during `spyViewing`. |
| Command envelope | Keep `commandId`, `dealId`, and `expectedRevision`. Add `powerId` for each power command and use the existing slot reference list. |

The client and server ship together. All power resolution goes through the state machine.

## TEST room

Put a **TEST room** entry on staging tabletop home. A board session creates a fresh private test room and shows a QR invite for one human phone. Fill the other three seats with labeled bots. The board still deals and starts, and all regular rooms keep their normal join flow.

| Setup option | Behavior |
| --- | --- |
| **Random play** | Use an optional seed to repeat the shuffle and bot choices for the same initial setup and human commands. Show the seed so a failed run can be reproduced. |
| **Spy drill** | Make the human the first player with a qualifying Spy card on the top of the deck. Go through the usual deal, opening look, draw, activate, and targeting steps. |
| **Swap drill** | Use the same flow with a qualifying Swap card. |

Bots acknowledge their initial previews automatically. In a TEST room, opening-look commands for the same deal can proceed across intervening bot readiness updates. The server still validates the acting seat, phase, and slots. On their turns, they choose from server-provided actions and valid selections using only their own player view. A bot can make an incorrect match. Bots never consult hidden ranks to pick moves or wait for input from the iPad. They also acknowledge private Spy looks. Pause briefly before each bot move so the board shows the sequence. Schedule at most one bot command at a time. Recheck room generation, revision, actor, and available actions after the delay and drop stale work. Bot moves pass through the same authorization and transition logic as human moves.

**Restart test** on the board recreates the selected scenario inside the existing room. Keep the board and human session, reset the round and scores, increment the room generation, and cancel pending bot work. Restart checks the test generation so bot turns cannot invalidate the reset request and an old request cannot reset a newer run. Retain the selected seed for **Restart test**. **Restart with new seed** is available for a fresh run. Keep room limits and expiry in force. Hide the TEST entry and reject test-only creation on environments where it is disabled.

## Acceptance checks

1. Run Spy twice with identical setup. In one run pick the opponent card on the phone, and in the other pick it on the iPad. Both paths show the same confirmation on the actor’s phone. Only the actor receives the rank, and **Done** removes it and advances once.
2. Run Swap with each target surface. Select the own card first, change it after staging a target, then confirm a new pair. Both hands update together. No device gets a rank through the Swap result.
3. Race target taps from both devices, duplicate confirmation, and send a stale target after the next turn. Verify that stale requests return the latest snapshot and a power resolves at most once.
4. Reload during own selection, target review, and the private Spy look. Revoke or disconnect the board during review and finish on the phone. An unrelated phone and a revoked board cannot act for the actor.
5. Verify that a finished power leaves the iPad read-only during ordinary play. Check board and non-actor snapshots for private ranks, private toasts, and actor-only controls.
6. Create a test room, join one phone by QR, check exactly three bots, and start from the iPad. Let bots take multiple turns, including a power and an opening look. Confirm the human turn stays paused until the human acts.
7. Repeat a seed with identical human commands, then restart while a bot is waiting. Verify repeatable moves, preserved sessions, and no bot action leaking into the new game.
