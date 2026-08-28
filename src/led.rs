//! Yesoul ambient LED strip protocol (vendor service `0xFFF0`).
//!
//! Wire contract from the official app (`writeCmdLight`, research 007 /
//! задача 058): write three raw bytes to characteristic `0xFFF2`. This is
//! **not** a FitShow frame (no `02 … xor 03` envelope) — keep it out of
//! `fitshow.rs`. No reply is expected on any notify characteristic.
//!
//! Never write `0xFF00` / `0xFF01` / `0xFAB*` from here. Never send
//! `F0 10 00` (the app's unverified "light reset" on connect).

// Callers land in later 058 commits; tests already exercise the items.
#![cfg_attr(not(test), allow(dead_code))]

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

/// Yesoul vendor LED service — `0xFFF0`.
pub const LED_SERVICE: Uuid = Uuid::from_u128(0x0000fff0_0000_1000_8000_00805f9b34fb);

/// LED write characteristic — `0xFFF2` (WriteWithoutResponse when advertised).
pub const LED_WRITE_CHAR: Uuid = Uuid::from_u128(0x0000fff2_0000_1000_8000_00805f9b34fb);

/// First byte of every ambient-light command (`writeCmdLight`).
const LED_FRAME_HEAD: u8 = 0xF0;
/// Light-command discriminator (second byte).
const LED_FRAME_CMD: u8 = 0x10;
/// Payload: strip on.
const LED_PAYLOAD_ON: u8 = 0x02;
/// Payload: strip off. Distinct from `0x00` (app connect "reset" — not sent).
const LED_PAYLOAD_OFF: u8 = 0x01;

/// Requested state of the W2 Pro ambient LED strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedState {
    On,
    Off,
}

impl fmt::Display for LedState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::On => "on",
            Self::Off => "off",
        })
    }
}

impl FromStr for LedState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "on" => Ok(Self::On),
            "off" => Ok(Self::Off),
            other => Err(format!("unknown LED state {other:?}; expected on or off")),
        }
    }
}

/// Three-byte LED command: `F0 10 02` (on) or `F0 10 01` (off).
#[must_use]
pub fn led_frame(state: LedState) -> [u8; 3] {
    match state {
        LedState::On => [LED_FRAME_HEAD, LED_FRAME_CMD, LED_PAYLOAD_ON],
        LedState::Off => [LED_FRAME_HEAD, LED_FRAME_CMD, LED_PAYLOAD_OFF],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_frame_is_f0_10_02() {
        assert_eq!(led_frame(LedState::On), [0xF0, 0x10, 0x02]);
    }

    #[test]
    fn off_frame_is_f0_10_01() {
        assert_eq!(led_frame(LedState::Off), [0xF0, 0x10, 0x01]);
    }

    #[test]
    fn frames_never_send_the_unverified_connect_reset() {
        // `F0 10 00` is the app's on-connect "light reset"; v1 must not emit it.
        let reset = [0xF0, 0x10, 0x00];
        assert_ne!(led_frame(LedState::On), reset);
        assert_ne!(led_frame(LedState::Off), reset);
    }

    #[test]
    fn display_and_fromstr_round_trip() {
        for state in [LedState::On, LedState::Off] {
            let parsed: LedState = state.to_string().parse().expect("round-trips");
            assert_eq!(parsed, state);
        }
        assert_eq!(LedState::On.to_string(), "on");
        assert_eq!(LedState::Off.to_string(), "off");
    }

    #[test]
    fn fromstr_rejects_garbage() {
        assert!("On".parse::<LedState>().is_err());
        assert!("OFF".parse::<LedState>().is_err());
        assert!("maybe".parse::<LedState>().is_err());
        assert!("".parse::<LedState>().is_err());
    }

    #[test]
    fn uuids_are_standard_16bit_aliases() {
        assert_eq!(
            LED_SERVICE.to_string(),
            "0000fff0-0000-1000-8000-00805f9b34fb"
        );
        assert_eq!(
            LED_WRITE_CHAR.to_string(),
            "0000fff2-0000-1000-8000-00805f9b34fb"
        );
    }
}
