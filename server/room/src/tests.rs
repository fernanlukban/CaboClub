use super::*;
use cabo_shared::protocol::{CardPile, CardTravel};

fn command(room: &Room, id: &str, action: Action, slots: Vec<u32>) -> Command {
    Command {
        command_id: id.into(),
        expected_revision: room.revision,
        action,
        slots,
        insert_left: false,
        deal_id: room.round.as_ref().map(|round| round.id),
        test_generation: room.test.as_ref().map(|test| test.generation),
        power_id: room
            .round
            .as_ref()
            .and_then(|round| round.step.power().map(|p| p.0)),
    }
}

fn run(room: &mut Room, role: Role, action: Action, slots: Vec<u32>) {
    let token = match role {
        Role::Seat(index) => room.players[index].token.clone(),
        Role::CreatorBoard => room.creator.clone().unwrap(),
        Role::Board => room.board.clone().unwrap(),
    };
    let cmd = command(room, &format!("revision-{}", room.revision), action, slots);
    room.command(&token, role, cmd).unwrap();
}

fn setup() -> Room {
    let mut room = Room::new("TESTROOM".into(), true);
    room.join("Alex", None).unwrap();
    room.join("Bea", None).unwrap();
    run(&mut room, Role::CreatorBoard, Action::DealRound, vec![]);
    room
}

fn start(room: &mut Room) {
    for index in 0..room.players.len() {
        let pair = slots(&room.players[index])[..2].to_vec();
        run(room, Role::Seat(index), Action::RevealInitialCards, pair);
        run(room, Role::Seat(index), Action::ConfirmInitialLook, vec![]);
    }
    let host = room.players[0].token.clone();
    let phone_start = command(room, "phone-start", Action::BeginPlay, vec![1]);
    assert_eq!(
        room.command(&host, Role::Seat(0), phone_start),
        Err("actionUnavailable")
    );
    run(room, Role::CreatorBoard, Action::BeginPlay, vec![1]);
    assert!(
        room.actions(Role::CreatorBoard)
            .iter()
            .all(|choice| matches!(
                choice.action,
                Action::RestartTest | Action::RestartTestNewSeed
            ))
    );
}

#[test]
fn board_cannot_start_early_and_confirmed_ranks_stay_hidden_on_replay() {
    let mut room = setup();
    let board = room.creator.clone().unwrap();
    let start = command(&room, "early", Action::BeginPlay, vec![1]);
    assert_eq!(
        room.command(&board, Role::CreatorBoard, start),
        Err("actionUnavailable")
    );
    let player = room.players[0].token.clone();
    let pair = slots(&room.players[0])[..2].to_vec();
    let reveal = command(&room, "reveal", Action::RevealInitialCards, pair);
    room.command(&player, Role::Seat(0), reveal.clone())
        .unwrap();
    assert_eq!(
        room.snapshot(Role::Seat(0)).players[0]
            .cards
            .iter()
            .filter(|card| card.rank.is_some())
            .count(),
        2
    );
    for role in [Role::CreatorBoard, Role::Seat(1)] {
        assert!(
            room.snapshot(role).players[0]
                .cards
                .iter()
                .all(|card| card.rank.is_none())
        );
    }
    run(&mut room, Role::Seat(0), Action::ConfirmInitialLook, vec![]);
    room.command(&player, Role::Seat(0), reveal).unwrap();
    assert!(
        room.snapshot(Role::Seat(0)).players[0]
            .cards
            .iter()
            .all(|card| card.rank.is_none())
    );
}

#[test]
fn board_stages_spy_but_only_actor_reveals_and_acknowledges() {
    let mut room = setup();
    start(&mut room);
    room.round.as_mut().unwrap().deck.last_mut().unwrap().rank = 9;
    run(&mut room, Role::Seat(0), Action::DrawDeck, vec![]);
    run(&mut room, Role::Seat(0), Action::UseDrawnPower, vec![]);
    let target = room.players[1].hand[0];
    let alternative = room.players[1].hand[1].slot;
    let actor = room.players[0].token.clone();
    let board = room.creator.clone().unwrap();
    let racing = command(&room, "race", Action::StagePowerTarget, vec![alternative]);
    run(
        &mut room,
        Role::CreatorBoard,
        Action::StagePowerTarget,
        vec![target.slot],
    );
    assert_eq!(
        room.command(&actor, Role::Seat(0), racing),
        Err("staleRevision")
    );
    let confirm = command(&room, "confirm", Action::ConfirmSpy, vec![target.slot]);
    assert_eq!(
        room.command(&board, Role::CreatorBoard, confirm.clone()),
        Err("actionUnavailable")
    );
    room.command(&actor, Role::Seat(0), confirm.clone())
        .unwrap();
    let public = room.snapshot(Role::CreatorBoard);
    assert_eq!(public.pending_power.unwrap().target_slot, Some(target.slot));
    assert!(public.private_power_result.is_none());
    assert!(
        room.snapshot(Role::Seat(1))
            .pending_power
            .unwrap()
            .target_slot
            .is_none()
    );
    let reveal = room.snapshot(Role::Seat(0)).private_power_result.unwrap();
    assert_eq!(
        (reveal.slot, reveal.rank),
        (Some(target.slot), Some(target.rank))
    );
    assert!(
        room.snapshot(Role::CreatorBoard)
            .private_power_result
            .is_none()
    );
    assert!(room.snapshot(Role::Seat(1)).private_power_result.is_none());
    assert!(room.actions(Role::CreatorBoard).is_empty());
    assert_eq!(room.round.as_ref().unwrap().turn, 0);
    run(&mut room, Role::Seat(0), Action::AcknowledgeSpy, vec![]);
    room.command(&actor, Role::Seat(0), confirm).unwrap();
    assert!(room.snapshot(Role::Seat(0)).private_power_result.is_none());
    assert_eq!(room.round.as_ref().unwrap().turn, 1);
}

#[test]
fn swap_requires_current_pair_and_is_applied_once() {
    let mut room = setup();
    start(&mut room);
    room.round.as_mut().unwrap().deck.last_mut().unwrap().rank = 11;
    run(&mut room, Role::Seat(0), Action::DrawDeck, vec![]);
    run(&mut room, Role::Seat(0), Action::UseDrawnPower, vec![]);
    let own = room.players[0].hand[0].slot;
    let target = room.players[1].hand[0].slot;
    let actor = room.players[0].token.clone();
    run(
        &mut room,
        Role::Seat(0),
        Action::SelectPowerOwnCard,
        vec![own],
    );
    run(
        &mut room,
        Role::CreatorBoard,
        Action::StagePowerTarget,
        vec![target],
    );
    let incorrect = command(&room, "reverse", Action::ConfirmSwap, vec![target, own]);
    assert_eq!(
        room.command(&actor, Role::Seat(0), incorrect),
        Err("invalidSelection")
    );
    let confirm = command(&room, "swap", Action::ConfirmSwap, vec![own, target]);
    room.command(&actor, Role::Seat(0), confirm.clone())
        .unwrap();
    room.command(&actor, Role::Seat(0), confirm).unwrap();
    assert_eq!(room.players[0].hand[0].slot, target);
    assert_eq!(room.players[1].hand[0].slot, own);
    assert_eq!(
        room.snapshot(Role::Seat(0))
            .private_power_result
            .unwrap()
            .kind,
        "swap"
    );
    assert!(
        room.snapshot(Role::CreatorBoard)
            .private_power_result
            .is_none()
    );
    assert_eq!(room.round.as_ref().unwrap().turn, 1);
}

#[test]
fn bot_room_restarts_with_original_seed_and_sessions() {
    let mut room = Room::new("BOTS".into(), true);
    room.enable_test(TestScenario::Spy, 42);
    let player = room.join("Avery", None).unwrap();
    let board = room.creator.clone().unwrap();
    assert_eq!(room.players.iter().filter(|player| player.bot).count(), 3);
    run(&mut room, Role::CreatorBoard, Action::DealRound, vec![]);
    let original: Vec<_> = room.players[0].hand.iter().map(|c| c.rank).collect();
    assert_eq!(room.round.as_ref().unwrap().deck.last().unwrap().rank, 9);
    for _ in 0..6 {
        room.tick_bot(Instant::now() + Duration::from_secs(1));
    }
    assert!(room.players[1..].iter().all(|bot| {
        room.round
            .as_ref()
            .unwrap()
            .opening
            .preview(bot.id)
            .is_some_and(|preview| matches!(preview, Preview::Confirmed(_)))
    }));
    assert_eq!(
        room.round.as_ref().unwrap().opening.preview(1),
        Some(Preview::Hidden)
    );
    run(&mut room, Role::CreatorBoard, Action::RestartTest, vec![]);
    room.tick_bot(Instant::now() + Duration::from_secs(1));
    assert_eq!(room.role(&board), Some(Role::CreatorBoard));
    assert_eq!(room.role(&player), Some(Role::Seat(0)));
    assert_eq!(room.test.as_ref().unwrap().generation, 2);
    run(&mut room, Role::CreatorBoard, Action::DealRound, vec![]);
    assert_eq!(
        room.players[0]
            .hand
            .iter()
            .map(|c| c.rank)
            .collect::<Vec<_>>(),
        original
    );
}

#[test]
fn explicit_seats_follow_counterclockwise_cabo_and_round_winner_starts() {
    let mut room = Room::new("COMPASS".into(), true);
    for (name, direction) in [
        ("East", Direction::East),
        ("South", Direction::South),
        ("North", Direction::North),
        ("West", Direction::West),
    ] {
        room.join(name, Some(direction)).unwrap();
    }
    assert_eq!(
        room.counterclockwise_seat_indexes(2).collect::<Vec<_>>(),
        [3, 1, 0, 2]
    );
    let ready = |room: &mut Room| {
        for index in 0..4 {
            let pair = slots(&room.players[index])[..2].to_vec();
            run(room, Role::Seat(index), Action::RevealInitialCards, pair);
            run(room, Role::Seat(index), Action::ConfirmInitialLook, vec![]);
        }
    };
    run(&mut room, Role::CreatorBoard, Action::DealRound, vec![]);
    ready(&mut room);
    let board = room.creator.clone().unwrap();
    let missing = command(&room, "missing", Action::BeginPlay, vec![]);
    assert_eq!(
        room.command(&board, Role::CreatorBoard, missing),
        Err("invalidSelection")
    );
    run(&mut room, Role::CreatorBoard, Action::BeginPlay, vec![3]);
    assert_eq!(room.round.as_ref().unwrap().turn, 2);
    run(&mut room, Role::Seat(2), Action::DrawDeck, vec![]);
    run(&mut room, Role::Seat(2), Action::DiscardDrawn, vec![]);
    assert_eq!(room.round.as_ref().unwrap().turn, 3);
    run(&mut room, Role::Seat(3), Action::CallCabo, vec![]);
    assert_eq!(
        room.round
            .as_ref()
            .unwrap()
            .final_turns
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [1, 0, 2]
    );
    for (index, player) in room.players.iter_mut().enumerate() {
        for card in &mut player.hand {
            card.rank = if index == 0 { 1 } else { 5 };
        }
    }
    for index in [1, 0, 2] {
        assert_eq!(room.round.as_ref().unwrap().turn, index);
        run(&mut room, Role::Seat(index), Action::DrawDeck, vec![]);
        run(&mut room, Role::Seat(index), Action::DiscardDrawn, vec![]);
    }
    assert_eq!(
        room.snapshot(Role::CreatorBoard).next_starter_seat_id,
        Some(1)
    );
    run(&mut room, Role::CreatorBoard, Action::DealNextRound, vec![]);
    ready(&mut room);
    run(&mut room, Role::CreatorBoard, Action::BeginPlay, vec![]);
    assert_eq!(room.round.as_ref().unwrap().turn, 0);
}

#[test]
fn assigned_compass_seats_are_unique_and_skip_open_directions() {
    let mut room = Room::new("TWO".into(), true);
    room.join("West", Some(Direction::West)).unwrap();
    assert_eq!(
        room.join("Another West", Some(Direction::West)),
        Err("seatTaken")
    );
    room.join("East", Some(Direction::East)).unwrap();
    assert_eq!(
        room.counterclockwise_seat_indexes(0).collect::<Vec<_>>(),
        [1, 0]
    );
    assert_eq!(
        room.snapshot(Role::CreatorBoard).players[0].direction,
        Direction::West
    );
}

#[test]
fn taking_last_discard_reveals_a_replacement_and_keeps_the_draw() {
    let mut room = setup();
    start(&mut room);
    let round = room.round.as_ref().unwrap();
    let taken = round.discard[0];
    let replacement = *round.deck.last().unwrap();
    let deck_count = round.deck.len();
    run(&mut room, Role::Seat(0), Action::DrawDiscard, vec![]);
    let round = room.round.as_ref().unwrap();
    assert_eq!(round.discard.last().unwrap().slot, replacement.slot);
    assert!(round.discard.last().unwrap().face_up);
    assert_eq!(round.deck.len(), deck_count - 1);
    assert_eq!(
        room.snapshot(Role::Seat(0)).incoming.unwrap().slot,
        taken.slot
    );
    let trace = room.game_log();
    let entry = trace.entries.back().unwrap();
    assert_eq!(entry.action, Some(Action::DrawDiscard));
    assert_eq!(entry.before.discard_count, 1);
    assert_eq!(entry.after.discard_count, 1);
}

#[test]
fn missed_match_first_refills_before_a_penalty_and_preserves_the_round() {
    let mut room = setup();
    start(&mut room);
    room.players[0].hand[0].rank = 2;
    room.players[0].hand[1].rank = 3;
    room.round.as_mut().unwrap().deck.truncate(1);
    let selected = slots(&room.players[0])[..3].to_vec();
    run(&mut room, Role::Seat(0), Action::CallMatch, selected);
    let deadline = room
        .round
        .as_ref()
        .unwrap()
        .pending_match
        .as_ref()
        .unwrap()
        .deadline;
    room.tick(deadline);
    let round = room.round.as_ref().unwrap();
    assert_eq!(round.discard.len(), 1);
    assert_eq!(room.players[0].hand.len(), 5);
    assert_eq!(room.snapshot(Role::CreatorBoard).phase, "roundResults");
}

#[test]
fn seeded_legal_turn_fuzz_keeps_cards_and_discard_intact() {
    for seed in 0..128 {
        let mut room = Room::new("FUZZ".into(), true);
        room.enable_test(TestScenario::Random, seed);
        room.join("Human", None).unwrap();
        run(&mut room, Role::CreatorBoard, Action::DealRound, vec![]);
        start(&mut room);
        let mut rng = StdRng::seed_from_u64(seed);
        for step in 0..400 {
            let round = room.round.as_ref().unwrap();
            assert!(!round.discard.is_empty(), "seed {seed}, step {step}");
            let incoming = match round.step {
                TurnStep::Incoming { card, .. } => Some(card.slot),
                TurnStep::MatchFlipping => round
                    .pending_match
                    .as_ref()
                    .unwrap()
                    .incoming
                    .map(|(card, _)| card.slot),
                _ => None,
            };
            let all_slots: Vec<_> = round
                .deck
                .iter()
                .chain(&round.discard)
                .chain(room.players.iter().flat_map(|player| &player.hand))
                .map(|card| card.slot)
                .chain(incoming)
                .collect();
            assert_eq!(all_slots.len(), 52, "seed {seed}, step {step}");
            assert_eq!(
                all_slots.into_iter().collect::<HashSet<_>>().len(),
                52,
                "seed {seed}, step {step}"
            );
            if room.phase != Phase::Playing {
                break;
            }
            if let Some(pending) = &round.pending_match {
                room.tick(pending.deadline);
                continue;
            }
            let role = Role::Seat(round.turn);
            let mut actions = room.actions(role);
            actions
                .retain(|choice| !matches!(choice.action, Action::PairBoard | Action::RevokeBoard));
            assert!(!actions.is_empty(), "seed {seed}, step {step}");
            let choice = actions.swap_remove(rng.random_range(0..actions.len()));
            let mut selected = choice.slots;
            if !matches!(choice.action, Action::ConfirmSpy | Action::ConfirmSwap) {
                selected.shuffle(&mut rng);
                selected.truncate(rng.random_range(choice.min..=choice.max));
            }
            run(&mut room, role, choice.action, selected);
        }
        assert!(
            room.phase != Phase::Playing,
            "seed {seed} got stuck in an open round"
        );
    }
}

#[test]
fn three_card_match_first_keeps_previous_discard_and_replays_once() {
    let mut room = setup();
    start(&mut room);
    let previous_discard = *room.round.as_ref().unwrap().discard.last().unwrap();
    for card in &mut room.players[0].hand[..3] {
        card.rank = 4;
    }
    let pair = slots(&room.players[0])[..3].to_vec();
    let token = room.players[0].token.clone();
    let match_cards = command(&room, "match", Action::CallMatch, pair);
    room.command(&token, Role::Seat(0), match_cards.clone())
        .unwrap();
    let flip = room.round.as_ref().unwrap().pending_match.as_ref().unwrap();
    let deadline = flip.deadline;
    let board = room.snapshot(Role::CreatorBoard);
    assert_eq!(board.match_flip.unwrap().slots.len(), 3);
    assert_eq!(board.players[0].cards.len(), 4);
    assert!(
        board.players[0].cards[..3]
            .iter()
            .all(|card| card.face_up && card.rank == Some(4))
    );
    assert_eq!(board.players[0].cards[3].rank, None);
    assert!(
        room.actions(Role::Seat(0))
            .iter()
            .all(|choice| !matches!(choice.action, Action::SkipTurn | Action::CallMatch))
    );
    room.tick(deadline - Duration::from_millis(1));
    assert_eq!(room.round.as_ref().unwrap().turn, 0);
    room.tick(deadline);
    assert_eq!(room.players[0].hand.len(), 2);
    assert!(room.players[0].hand[0].face_up);
    assert_eq!(room.players[0].hand[0].slot, previous_discard.slot);
    assert_eq!(room.round.as_ref().unwrap().discard.last().unwrap().rank, 4);
    assert_eq!(room.round.as_ref().unwrap().turn, 1);
    for role in [Role::CreatorBoard, Role::Seat(0), Role::Seat(1)] {
        assert_eq!(
            room.snapshot(role).card_travel,
            Some(CardTravel {
                revision: room.revision,
                slot: previous_discard.slot,
                from: CardPile::Discard,
                to_seat_id: room.players[0].id,
            })
        );
    }
    let revision = room.revision;
    room.command(&token, Role::Seat(0), match_cards).unwrap();
    assert_eq!(room.revision, revision);
    run(&mut room, Role::Seat(1), Action::DrawDeck, vec![]);
    assert_eq!(
        room.snapshot(Role::CreatorBoard)
            .card_travel
            .unwrap()
            .revision,
        revision
    );
    assert!(room.snapshot(Role::Seat(0)).match_outcome.is_none());
}

#[test]
fn peek_stays_private_until_done_even_on_the_last_draw() {
    let mut room = setup();
    start(&mut room);
    let round = room.round.as_mut().unwrap();
    let mut draw = round.deck.pop().unwrap();
    draw.rank = 7;
    round.deck = vec![draw];
    run(&mut room, Role::Seat(0), Action::DrawDeck, vec![]);
    run(&mut room, Role::Seat(0), Action::UseDrawnPower, vec![]);
    let target = room.players[0].hand[0];
    run(&mut room, Role::Seat(0), Action::Peek, vec![target.slot]);
    let actor = room.snapshot(Role::Seat(0));
    assert_eq!(actor.phase, "playing");
    let reveal = actor.private_power_result.unwrap();
    assert_eq!(
        (reveal.slot, reveal.rank),
        (Some(target.slot), Some(target.rank))
    );
    for role in [Role::Seat(1), Role::CreatorBoard] {
        assert!(room.snapshot(role).private_power_result.is_none());
    }
    run(&mut room, Role::Seat(0), Action::AcknowledgePeek, vec![]);
    let actor = room.snapshot(Role::Seat(0));
    assert_eq!(actor.phase, "roundResults");
    assert!(actor.private_power_result.is_none());
}

#[test]
fn host_can_start_after_revoke_and_skip_a_pending_discard_pickup() {
    let mut room = setup();
    for index in 0..2 {
        let pair = slots(&room.players[index])[..2].to_vec();
        run(
            &mut room,
            Role::Seat(index),
            Action::RevealInitialCards,
            pair,
        );
        run(
            &mut room,
            Role::Seat(index),
            Action::ConfirmInitialLook,
            vec![],
        );
    }
    run(&mut room, Role::Seat(0), Action::RevokeBoard, vec![]);
    run(&mut room, Role::Seat(0), Action::BeginPlay, vec![1]);
    run(&mut room, Role::Seat(0), Action::CallCabo, vec![]);
    let discard = room.round.as_ref().unwrap().discard.last().unwrap().slot;
    let hand = slots(&room.players[1]);
    run(&mut room, Role::Seat(1), Action::DrawDiscard, vec![]);
    let guest = room.players[1].token.clone();
    let unauthorized = command(&room, "guest-skip", Action::SkipTurn, vec![]);
    assert_eq!(
        room.command(&guest, Role::Seat(1), unauthorized),
        Err("actionUnavailable")
    );
    let host = room.players[0].token.clone();
    let skip = command(&room, "host-skip", Action::SkipTurn, vec![]);
    room.command(&host, Role::Seat(0), skip.clone()).unwrap();
    assert_eq!(room.snapshot(Role::Seat(0)).phase, "roundResults");
    assert_eq!(
        room.round.as_ref().unwrap().discard.last().unwrap().slot,
        discard
    );
    assert_eq!(slots(&room.players[1]), hand);
    let revision = room.revision;
    room.command(&host, Role::Seat(0), skip).unwrap();
    assert_eq!(room.revision, revision);
}

#[test]
fn two_card_miss_keeps_incoming_without_adding_a_penalty() {
    let mut room = setup();
    start(&mut room);
    room.players[0].hand[0].rank = 4;
    room.players[0].hand[1].rank = 5;
    let pair = slots(&room.players[0])[..2].to_vec();
    run(&mut room, Role::Seat(0), Action::CallMatch, pair);
    let deadline = room
        .round
        .as_ref()
        .unwrap()
        .pending_match
        .as_ref()
        .unwrap()
        .deadline;
    room.tick(deadline);
    let board = room.snapshot(Role::CreatorBoard);
    assert!(board.match_flip.is_none());
    assert_eq!(
        board.players[0]
            .cards
            .iter()
            .map(|card| card.rank)
            .collect::<Vec<_>>(),
        vec![
            Some(4),
            Some(5),
            None,
            None,
            Some(room.players[0].hand[4].rank)
        ]
    );
    let result = room.snapshot(Role::Seat(0)).match_outcome.unwrap();
    assert!(!result.matched);
    assert!(result.penalty_slot.is_none());
    assert!(result.discarded_slots.is_empty());
    assert_eq!(
        room.players[0].hand.last().unwrap().slot,
        result.incoming_slot
    );
    assert!(room.snapshot(Role::Seat(1)).match_outcome.is_none());
    assert!(room.snapshot(Role::CreatorBoard).match_outcome.is_none());
}

#[test]
fn board_marks_opening_slots_without_ranks_until_first_move() {
    let mut room = setup();
    start(&mut room);
    let board = room.snapshot(Role::CreatorBoard);
    for player in &board.players {
        let expected = [player.cards[0].slot, player.cards[1].slot];
        assert_eq!(player.opening_look_slots, Some(expected));
        assert!(player.cards.iter().all(|card| card.rank.is_none()));
    }
    assert!(
        room.snapshot(Role::Seat(0))
            .players
            .iter()
            .all(|player| player.opening_look_slots.is_none())
    );
    run(&mut room, Role::Seat(0), Action::DrawDeck, vec![]);
    assert!(
        room.snapshot(Role::CreatorBoard)
            .players
            .iter()
            .all(|player| player.opening_look_slots.is_none())
    );
}

#[test]
fn face_up_discard_stays_public_when_picked_up_and_kept() {
    let mut room = setup();
    start(&mut room);
    let discard = room.round.as_ref().unwrap().discard.last().unwrap().rank;
    run(&mut room, Role::Seat(0), Action::DrawDiscard, vec![]);
    let board = room.snapshot(Role::CreatorBoard);
    assert_eq!(board.incoming_source, Some("discard"));
    let incoming = board.incoming.unwrap();
    assert!(incoming.face_up);
    assert_eq!(incoming.rank, Some(discard));
    let target = room.players[0].hand[0].slot;
    run(
        &mut room,
        Role::Seat(0),
        Action::DiscardHandCard,
        vec![target],
    );
    let kept = &room.snapshot(Role::Seat(1)).players[0].cards[0];
    assert!(kept.face_up);
    assert_eq!(kept.rank, Some(discard));
    run(&mut room, Role::Seat(1), Action::DrawDeck, vec![]);
    let board = room.snapshot(Role::CreatorBoard);
    assert_eq!(board.incoming_source, Some("deck"));
    assert!(board.incoming.is_none());
    assert!(!room.snapshot(Role::Seat(1)).incoming.unwrap().face_up);
}

#[test]
fn cabo_scores_once_after_the_other_player_finishes() {
    let mut room = setup();
    start(&mut room);
    run(&mut room, Role::Seat(0), Action::CallCabo, vec![]);
    run(&mut room, Role::Seat(1), Action::DrawDeck, vec![]);
    let token = room.players[1].token.clone();
    let finish = command(&room, "finish", Action::DiscardDrawn, vec![]);
    room.command(&token, Role::Seat(1), finish.clone()).unwrap();
    assert_eq!(room.snapshot(Role::CreatorBoard).phase, "roundResults");
    let scores: Vec<_> = room.players.iter().map(|player| player.score).collect();
    room.command(&token, Role::Seat(1), finish).unwrap();
    assert_eq!(
        room.players
            .iter()
            .map(|player| player.score)
            .collect::<Vec<_>>(),
        scores
    );
}
