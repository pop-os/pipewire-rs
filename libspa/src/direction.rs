// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! SPA direction.

/// A port direction.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Direction {
    /// Input
    Input,
    /// Output
    Output,
}

impl Direction {
    /// The raw representation of the direction
    pub fn as_raw(&self) -> spa_sys::spa_direction {
        match self {
            Self::Input => spa_sys::SPA_DIRECTION_INPUT,
            Self::Output => spa_sys::SPA_DIRECTION_OUTPUT,
        }
    }

    /// Create a `Direction` from a raw `spa_direction`.
    ///
    /// # Panics
    /// This function will panic if `raw` is an invalid direction.
    pub fn from_raw(raw: spa_sys::spa_direction) -> Self {
        match raw {
            spa_sys::SPA_DIRECTION_INPUT => Self::Input,
            spa_sys::SPA_DIRECTION_OUTPUT => Self::Output,
            _ => panic!("Invalid direction: {}", raw),
        }
    }

    /// Return a new [`Direction`] in the opposite direction, turning Input to Output, and Output to Input.
    pub fn reverse(&self) -> Self {
        match self {
            Self::Input => Self::Output,
            Self::Output => Self::Input,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_raw() {
        assert_eq!(Direction::Input.as_raw(), spa_sys::SPA_DIRECTION_INPUT);
        assert_eq!(Direction::Output.as_raw(), spa_sys::SPA_DIRECTION_OUTPUT);
    }

    #[test]
    fn from_raw() {
        assert_eq!(
            Direction::Input,
            Direction::from_raw(spa_sys::SPA_DIRECTION_INPUT)
        );
        assert_eq!(
            Direction::Output,
            Direction::from_raw(spa_sys::SPA_DIRECTION_OUTPUT)
        );
    }

    #[test]
    #[should_panic]
    fn invalid_direction() {
        Direction::from_raw(u32::MAX);
    }

    #[test]
    fn reverse() {
        assert_eq!(Direction::Output.reverse(), Direction::Input);
        assert_eq!(Direction::Input.reverse(), Direction::Output);
    }
}
