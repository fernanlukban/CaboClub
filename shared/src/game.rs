pub const MIN_PLAYERS: usize = 2;
pub const MAX_PLAYERS: usize = 4;
pub const STARTING_HAND_SIZE: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawSource {
    Deck,
    Discard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawError {
    RoundInactive,
    NotYourTurn,
    MoveInProgress,
    PileEmpty,
}

/// Facts needed to validate a draw. Clients derive them from their own view.
/// The server must derive them from its authoritative game state.
#[derive(Debug, Clone, Copy)]
pub struct DrawContext {
    pub round_active: bool,
    pub is_current_player: bool,
    pub move_in_progress: bool,
    pub deck_has_cards: bool,
    pub discard_has_cards: bool,
}

impl DrawContext {
    pub fn validate(self, source: DrawSource) -> Result<(), DrawError> {
        if !self.round_active {
            return Err(DrawError::RoundInactive);
        }
        if !self.is_current_player {
            return Err(DrawError::NotYourTurn);
        }
        if self.move_in_progress {
            return Err(DrawError::MoveInProgress);
        }
        let available = match source {
            DrawSource::Deck => self.deck_has_cards,
            DrawSource::Discard => self.discard_has_cards,
        };
        if !available {
            return Err(DrawError::PileEmpty);
        }
        Ok(())
    }
}
