use crate::game::{MAX_PLAYERS, MIN_PLAYERS};

type SeatId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    InitialPreview,
    ReadyToStart,
    Playing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preview {
    Hidden,
    Revealed,
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupError {
    InvalidRoster,
    UnknownSeat,
    InvalidTransition,
    NotReady,
}

/// Opening state for one deal. The caller verifies session authority and
/// deal identity before invoking a transition.
#[derive(Debug)]
pub struct RoundStart {
    previews: Vec<(SeatId, Preview)>,
    phase: Phase,
}

impl RoundStart {
    pub fn new(seats: &[SeatId]) -> Result<Self, SetupError> {
        if !(MIN_PLAYERS..=MAX_PLAYERS).contains(&seats.len()) {
            return Err(SetupError::InvalidRoster);
        }

        let mut previews = Vec::with_capacity(seats.len());
        for &seat in seats {
            if previews.iter().any(|&(id, _)| id == seat) {
                return Err(SetupError::InvalidRoster);
            }
            previews.push((seat, Preview::Hidden));
        }

        Ok(Self {
            previews,
            phase: Phase::InitialPreview,
        })
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn preview(&self, seat: SeatId) -> Option<Preview> {
        self.previews
            .iter()
            .find_map(|&(id, preview)| (id == seat).then_some(preview))
    }

    pub fn reveal(&mut self, seat: SeatId) -> Result<(), SetupError> {
        self.transition_preview(seat, Preview::Hidden, Preview::Revealed)
    }

    pub fn confirm(&mut self, seat: SeatId) -> Result<(), SetupError> {
        self.transition_preview(seat, Preview::Revealed, Preview::Confirmed)?;
        if self
            .previews
            .iter()
            .all(|&(_, preview)| preview == Preview::Confirmed)
        {
            self.phase = Phase::ReadyToStart;
        }
        Ok(())
    }

    pub fn begin_play(&mut self) -> Result<(), SetupError> {
        match self.phase {
            Phase::InitialPreview => Err(SetupError::NotReady),
            Phase::Playing => Err(SetupError::InvalidTransition),
            Phase::ReadyToStart => {
                self.phase = Phase::Playing;
                Ok(())
            }
        }
    }

    fn transition_preview(
        &mut self,
        seat: SeatId,
        expected: Preview,
        next: Preview,
    ) -> Result<(), SetupError> {
        if self.phase != Phase::InitialPreview {
            return Err(SetupError::InvalidTransition);
        }
        let (_, preview) = self
            .previews
            .iter_mut()
            .find(|(id, _)| *id == seat)
            .ok_or(SetupError::UnknownSeat)?;
        if *preview != expected {
            return Err(SetupError::InvalidTransition);
        }
        *preview = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_every_seat_to_reveal_and_confirm_before_start() {
        let mut setup = RoundStart::new(&[10, 20]).unwrap();

        assert_eq!(setup.reveal(30), Err(SetupError::UnknownSeat));
        assert_eq!(setup.confirm(10), Err(SetupError::InvalidTransition));
        setup.reveal(10).unwrap();
        assert_eq!(setup.preview(10), Some(Preview::Revealed));
        assert_eq!(setup.preview(20), Some(Preview::Hidden));

        setup.confirm(10).unwrap();
        assert_eq!(setup.preview(10), Some(Preview::Confirmed));
        assert_eq!(setup.confirm(10), Err(SetupError::InvalidTransition));
        assert_eq!(setup.begin_play(), Err(SetupError::NotReady));
        assert_eq!(setup.phase(), Phase::InitialPreview);

        setup.reveal(20).unwrap();
        assert_eq!(setup.begin_play(), Err(SetupError::NotReady));
        setup.confirm(20).unwrap();
        assert_eq!(setup.phase(), Phase::ReadyToStart);
        setup.begin_play().unwrap();
        assert_eq!(setup.phase(), Phase::Playing);
        assert_eq!(setup.begin_play(), Err(SetupError::InvalidTransition));
    }

    #[test]
    fn rejects_duplicate_seats_and_invalid_roster_sizes() {
        for seats in [&[10][..], &[10, 10], &[10, 20, 30, 40, 50]] {
            assert_eq!(
                RoundStart::new(seats).unwrap_err(),
                SetupError::InvalidRoster
            );
        }
    }
}
