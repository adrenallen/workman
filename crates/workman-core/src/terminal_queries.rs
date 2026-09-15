//! Host-owned default-color queries, available before any terminal viewer attaches.

use std::sync::{Arc, RwLock};

use alacritty_terminal::vte::ansi::{self, Handler, NamedColor, Rgb};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TerminalColors {
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub cursor: [u8; 3],
}

impl Default for TerminalColors {
    fn default() -> Self {
        // Desktop's Graphite theme, also used for launches before a desktop connects.
        Self {
            foreground: [0xd7, 0xd9, 0xd5],
            background: [0x20, 0x23, 0x26],
            cursor: [0xa7, 0xc7, 0xb7],
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SharedTerminalColors(Arc<RwLock<TerminalColors>>);

impl SharedTerminalColors {
    pub fn get(&self) -> TerminalColors {
        *self.0.read().unwrap_or_else(|error| error.into_inner())
    }

    pub fn set(&self, colors: TerminalColors) {
        *self.0.write().unwrap_or_else(|error| error.into_inner()) = colors;
    }
}

struct ColorState {
    defaults: SharedTerminalColors,
    overrides: [Option<Rgb>; 3],
    answering: bool,
    replies: Vec<Vec<u8>>,
}

fn color_slot(index: usize) -> Option<usize> {
    [
        NamedColor::Foreground,
        NamedColor::Background,
        NamedColor::Cursor,
    ]
    .iter()
    .position(|color| *color as usize == index)
}

impl Handler for ColorState {
    fn set_color(&mut self, index: usize, color: Rgb) {
        if let Some(slot) = color_slot(index) {
            self.overrides[slot] = Some(color);
        }
    }

    fn reset_color(&mut self, index: usize) {
        if let Some(slot) = color_slot(index) {
            self.overrides[slot] = None;
        }
    }

    fn reset_state(&mut self) {
        self.overrides = [None; 3];
    }

    fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str) {
        if !self.answering {
            return;
        }
        let Some(slot) = color_slot(index) else {
            return;
        };
        let defaults = self.defaults.get();
        let [r, g, b] = [defaults.foreground, defaults.background, defaults.cursor][slot];
        let Rgb { r, g, b } = self.overrides[slot].unwrap_or(Rgb { r, g, b });
        self.replies.push(
            format!("\x1b]{prefix};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}{terminator}")
                .into_bytes(),
        );
    }
}

/// Consume only the exact OSC 10/11/12 queries that this host answers. Keeping them
/// out of retained output prevents duplicate replies from xterm or `wrk attach`,
/// including when old output is replayed. Setters and all other traffic pass through.
/// The ANSI parser tracks application color overrides and string boundaries; it
/// never answers DA/DSR or other viewer-owned queries. At most seven bytes are held.
pub(crate) struct TerminalColorQueries {
    held: Vec<u8>,
    parser: ansi::Processor<ImmediateQueries>,
    state: ColorState,
}

// Synchronized painting must not delay a protocol reply: the application may be
// waiting for it before it ends the synchronized frame.
#[derive(Default)]
struct ImmediateQueries;

impl ansi::Timeout for ImmediateQueries {
    fn set_timeout(&mut self, _: std::time::Duration) {}
    fn clear_timeout(&mut self) {}
    fn pending_timeout(&self) -> bool {
        false
    }
}

impl TerminalColorQueries {
    const QUERIES: [&'static [u8]; 6] = [
        b"\x1b]10;?\x07",
        b"\x1b]10;?\x1b\\",
        b"\x1b]11;?\x07",
        b"\x1b]11;?\x1b\\",
        b"\x1b]12;?\x07",
        b"\x1b]12;?\x1b\\",
    ];

    pub(crate) fn new(defaults: SharedTerminalColors) -> Self {
        Self {
            held: Vec::new(),
            parser: ansi::Processor::new(),
            state: ColorState {
                defaults,
                overrides: [None; 3],
                answering: false,
                replies: Vec::new(),
            },
        }
    }

    pub(crate) fn filter(&mut self, bytes: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
        let mut buffer = std::mem::take(&mut self.held);
        buffer.extend_from_slice(bytes);
        let mut output = Vec::with_capacity(buffer.len());
        let mut index = 0;
        let mut plain_start = 0;
        while index < buffer.len() {
            if buffer[index] != 0x1b {
                index += 1;
                continue;
            }
            let remaining = &buffer[index..];
            if let Some(query) = Self::QUERIES
                .iter()
                .find(|query| remaining.starts_with(query))
            {
                self.parser
                    .advance(&mut self.state, &buffer[plain_start..index]);
                output.extend_from_slice(&buffer[plain_start..index]);
                let before = self.state.replies.len();
                self.state.answering = true;
                self.parser.advance(&mut self.state, query);
                self.state.answering = false;
                // A matching byte string inside another escape sequence isn't a query.
                if self.state.replies.len() == before {
                    output.extend_from_slice(query);
                }
                index += query.len();
                plain_start = index;
            } else if Self::QUERIES
                .iter()
                .any(|query| query.starts_with(remaining))
            {
                self.held.extend_from_slice(remaining);
                break;
            } else {
                index += 1;
            }
        }
        self.parser
            .advance(&mut self.state, &buffer[plain_start..index]);
        output.extend_from_slice(&buffer[plain_start..index]);
        (output, std::mem::take(&mut self.state.replies))
    }

    pub(crate) fn flush(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_colors_are_answered_once_without_a_viewer_at_every_read_boundary() {
        let source = b"before\x1b]10;?\x1b\\\x1b]11;?\x07\x1b]12;?\x07after\x1b[6n\x1b[c";
        for split in 0..=source.len() {
            let mut filter = TerminalColorQueries::new(SharedTerminalColors::default());
            let (mut output, mut replies) = filter.filter(&source[..split]);
            let (tail, more) = filter.filter(&source[split..]);
            output.extend(tail);
            output.extend(filter.flush());
            replies.extend(more);
            assert_eq!(output, b"beforeafter\x1b[6n\x1b[c");
            assert_eq!(
                replies,
                vec![
                    b"\x1b]10;rgb:d7d7/d9d9/d5d5\x1b\\".to_vec(),
                    b"\x1b]11;rgb:2020/2323/2626\x07".to_vec(),
                    b"\x1b]12;rgb:a7a7/c7c7/b7b7\x07".to_vec(),
                ]
            );
            // Replaying the retained bytes cannot elicit any second color answer.
            assert!(filter.filter(&output).1.is_empty());
        }
    }

    #[test]
    fn live_theme_changes_and_application_color_setters_are_respected_in_order() {
        let colors = SharedTerminalColors::default();
        let mut filter = TerminalColorQueries::new(colors.clone());
        let setters = b"\x1b]11;#123456\x07";
        let source = [
            setters.as_slice(),
            b"\x1b]11;?\x07\x1b]111\x07\x1b]11;?\x07",
        ]
        .concat();
        let (output, replies) = filter.filter(&source);
        assert_eq!(output, [setters.as_slice(), b"\x1b]111\x07"].concat());
        assert_eq!(replies[0], b"\x1b]11;rgb:1212/3434/5656\x07");
        assert_eq!(replies[1], b"\x1b]11;rgb:2020/2323/2626\x07");
        colors.set(TerminalColors {
            background: [241, 239, 232],
            ..colors.get()
        });
        assert_eq!(
            filter.filter(b"\x1b]11;?\x07").1[0],
            b"\x1b]11;rgb:f1f1/efef/e8e8\x07"
        );
    }

    #[test]
    fn other_osc_queries_text_and_incomplete_prefixes_remain_lossless() {
        let source = b"\x1b]4;2;?\x07\x1b]10;?;?\x07\x1b]0;title\x07\x1b[6nhello\x1b]11;?";
        let mut filter = TerminalColorQueries::new(SharedTerminalColors::default());
        let mut output = Vec::new();
        for byte in source {
            let (part, replies) = filter.filter(&[*byte]);
            output.extend(part);
            assert!(replies.is_empty());
            assert!(filter.held.len() <= 7);
        }
        output.extend(filter.flush());
        assert_eq!(output, source);
    }

    #[test]
    fn synchronized_painting_does_not_delay_color_answers() {
        let mut filter = TerminalColorQueries::new(SharedTerminalColors::default());
        let (output, replies) = filter.filter(b"\x1b[?2026h\x1b]11;?\x07");
        assert_eq!(output, b"\x1b[?2026h");
        assert_eq!(replies.len(), 1);
    }
}
