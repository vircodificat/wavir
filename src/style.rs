use std::error::Error;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Deserializer, de};

/// Visual style configuration, loaded from `style.json`.
///
/// All lengths are in virtual pixels: `signal_viewport_height` is the height
/// of the viewport, which maps onto the screen vertically.
#[derive(Debug, Clone, Deserialize)]
pub struct Style {
    pub rail: u32,
    pub signal_zone: u32,
    pub gap: u32,

    pub name_width: u32,

    /// Left padding of waveform labels, in virtual pixels.
    pub label_inset: u32,

    /// Font size of waveform labels, in virtual pixels.
    pub label_fontsize: u32,

    /// Color of the "now" cursor line, as a `#rrggbb` hex string.
    #[serde(deserialize_with = "deserialize_hex_color")]
    pub now_cursor_color: [u8; 3],

    /// Color of the waveform lines and bus rails, as a `#rrggbb` hex string.
    #[serde(deserialize_with = "deserialize_hex_color")]
    pub signal_color: [u8; 3],

    /// Height of the signal viewport, in virtual pixels.
    pub signal_viewport_height: u32,

    /// Uniform margin around the viewport on all four sides, in virtual
    /// pixels. The viewport is inset by this much from the screen edges.
    pub viewport_border: u32,

    /// Color of the viewport border rectangle, as a `#rrggbb` hex string.
    #[serde(deserialize_with = "deserialize_hex_color")]
    pub viewport_border_color: [u8; 3],

    /// Height of the time ruler at the top, in virtual pixels. Lanes are
    /// shifted down by this much to make space for it.
    pub time_ruler_height: u32,

    /// Length of the time ruler's tick marks, drawn downward from the top of
    /// the screen, in virtual pixels.
    pub time_ruler_tick_height: u32,
}

impl Style {
    /// Total height of one lane: `signal_zone + 2 * rail`.
    pub fn lane_height(&self) -> u32 {
        self.signal_zone + 2 * self.rail
    }

    /// Load a style from a JSON file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let text = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }
}

/// Deserialize a `#rrggbb` hex string into `[r, g, b]`.
fn deserialize_hex_color<'de, D>(d: D) -> Result<[u8; 3], D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    let hex = s.trim().strip_prefix('#').unwrap_or(&s);
    if hex.len() != 6 {
        return Err(de::Error::custom(format!(
            "expected #rrggbb hex color, got {s:?}"
        )));
    }
    let channel = |range: std::ops::Range<usize>| {
        u8::from_str_radix(&hex[range], 16)
            .map_err(|_| de::Error::custom(format!("invalid hex color {s:?}")))
    };
    Ok([channel(0..2)?, channel(2..4)?, channel(4..6)?])
}
