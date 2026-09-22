//! Self-contained ANSI checkpoints for terminal replay.

use std::fmt::Write;

use alacritty_terminal::{
    Term,
    event::VoidListener,
    grid::Dimensions,
    index::{Column, Line},
    term::{
        TermMode,
        cell::{Cell, Flags},
    },
    vte::ansi::{Color, NamedColor},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TerminalCheckpoint {
    pub rows: u16,
    pub columns: u16,
    /// Raw stream offset immediately following the state represented here.
    pub offset: u64,
    pub ansi: String,
}

pub(crate) fn primary_checkpoint(
    term: &Term<VoidListener>,
    offset: u64,
    scroll_region: Option<&str>,
) -> Option<TerminalCheckpoint> {
    // The saved primary grid is private inside Alacritty while a TUI is active.
    if term.mode().contains(TermMode::ALT_SCREEN) {
        return None;
    }
    Some(screen_checkpoint(term, offset, scroll_region, None))
}

/// Rebuild both buffers in the same order a terminal created them. `primary` is
/// captured before Alacritty swaps away its private primary grid.
pub(crate) fn alternate_checkpoint(
    term: &Term<VoidListener>,
    offset: u64,
    scroll_region: Option<&str>,
    primary: &str,
) -> Option<TerminalCheckpoint> {
    if !term.mode().contains(TermMode::ALT_SCREEN) {
        return None;
    }
    Some(screen_checkpoint(
        term,
        offset,
        scroll_region,
        Some(primary),
    ))
}

fn screen_checkpoint(
    term: &Term<VoidListener>,
    offset: u64,
    scroll_region: Option<&str>,
    primary: Option<&str>,
) -> TerminalCheckpoint {
    let grid = term.grid();
    let mut ansi = if let Some(primary) = primary {
        format!("{primary}\x1b[?1049h\x1b[?6l\x1b[r\x1b[?25l\x1b[H")
    } else {
        String::from("\x1b[?2026l\x1bc\x1b[?25l")
    };
    for index in 0..=NamedColor::Cursor as usize {
        if let Some(rgb) = term.colors()[index] {
            let command = if index < 256 {
                format!("4;{index}")
            } else {
                (index - 246).to_string()
            };
            let _ = write!(
                ansi,
                "\x1b]{command};rgb:{:02x}/{:02x}/{:02x}\x1b\\",
                rgb.r, rgb.g, rgb.b
            );
        }
    }
    let mut previous_style = None;
    for line in -(grid.history_size() as i32)..grid.screen_lines() as i32 {
        let row = &grid[Line(line)];
        let wrapped = row[Column(grid.columns() - 1)]
            .flags
            .contains(Flags::WRAPLINE);
        let mut text_end = if wrapped {
            grid.columns()
        } else {
            (0..grid.columns())
                .rfind(|&column| {
                    let cell = &row[Column(column)];
                    cell.c != ' '
                        || !cell.zerowidth().unwrap_or_default().is_empty()
                        || cell.fg != Color::Named(NamedColor::Foreground)
                        || !cell.flags.is_empty()
                        || cell.extra.is_some()
                })
                .map_or(0, |column| column + 1)
        };
        if line > -(grid.history_size() as i32)
            && grid[Line(line - 1)][Column(grid.columns() - 1)]
                .flags
                .contains(Flags::WRAPLINE)
        {
            // A printable cell commits the previous row's pending wrap.
            text_end = text_end.max(1);
        }
        for column in 0..text_end {
            let cell = &row[Column(column)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let style = (
                cell.fg,
                cell.bg,
                cell.flags & STYLE_FLAGS,
                cell.underline_color(),
                cell.hyperlink(),
            );
            if previous_style.as_ref() != Some(&style) {
                write_style(&mut ansi, cell);
                previous_style = Some(style);
            }
            ansi.push(if cell.c == '\t' { ' ' } else { cell.c });
            ansi.extend(cell.zerowidth().unwrap_or_default());
        }
        // Erased padding must stay blank cells. Printing spaces all the way to
        // the margin makes xterm reflow that padding into spurious extra lines
        // on the next resize. Preserve every background segment with ECH.
        let mut column = text_end;
        while column < grid.columns() {
            let cell = &row[Column(column)];
            let end = (column + 1..grid.columns())
                .find(|&next| row[Column(next)].bg != cell.bg)
                .unwrap_or(grid.columns());
            let style = (
                cell.fg,
                cell.bg,
                cell.flags & STYLE_FLAGS,
                cell.underline_color(),
                cell.hyperlink(),
            );
            if previous_style.as_ref() != Some(&style) {
                write_style(&mut ansi, cell);
                previous_style = Some(style);
            }
            let _ = write!(ansi, "\x1b[{}X", end - column);
            if end < grid.columns() {
                let _ = write!(ansi, "\x1b[{}C", end - column);
            }
            column = end;
        }
        if line + 1 < grid.screen_lines() as i32 && !wrapped {
            ansi.push_str("\r\n");
        }
    }
    if let Some(region) = scroll_region {
        ansi.push_str(region);
    }
    let origin_top = if term.mode().contains(TermMode::ORIGIN) {
        ansi.push_str("\x1b[?6h");
        scroll_region
            .and_then(|region| region.strip_prefix("\x1b["))
            .and_then(|region| region.trim_end_matches('r').split(';').next())
            .and_then(|top| top.parse::<i32>().ok())
            .unwrap_or(1)
            - 1
    } else {
        0
    };
    write_cursor(&mut ansi, &grid.saved_cursor, grid, origin_top);
    ansi.push_str("\x1b7");
    write_cursor(&mut ansi, &grid.cursor, grid, origin_top);
    for (mode, number) in [
        (TermMode::SHOW_CURSOR, 25),
        (TermMode::APP_CURSOR, 1),
        (TermMode::LINE_WRAP, 7),
        (TermMode::BRACKETED_PASTE, 2004),
        (TermMode::FOCUS_IN_OUT, 1004),
        (TermMode::MOUSE_REPORT_CLICK, 1000),
        (TermMode::MOUSE_DRAG, 1002),
        (TermMode::MOUSE_MOTION, 1003),
        (TermMode::UTF8_MOUSE, 1005),
        (TermMode::SGR_MOUSE, 1006),
    ] {
        let action = if term.mode().contains(mode) { 'h' } else { 'l' };
        let _ = write!(ansi, "\x1b[?{number}{action}");
    }
    for (mode, number) in [(TermMode::INSERT, 4), (TermMode::LINE_FEED_NEW_LINE, 20)] {
        let action = if term.mode().contains(mode) { 'h' } else { 'l' };
        let _ = write!(ansi, "\x1b[{number}{action}");
    }
    ansi.push_str(if term.mode().contains(TermMode::APP_KEYPAD) {
        "\x1b="
    } else {
        "\x1b>"
    });
    TerminalCheckpoint {
        rows: grid.screen_lines() as u16,
        columns: grid.columns() as u16,
        offset,
        ansi,
    }
}

const STYLE_FLAGS: Flags = Flags::from_bits_retain(
    Flags::BOLD.bits()
        | Flags::DIM.bits()
        | Flags::ITALIC.bits()
        | Flags::ALL_UNDERLINES.bits()
        | Flags::INVERSE.bits()
        | Flags::HIDDEN.bits()
        | Flags::STRIKEOUT.bits(),
);

fn write_style(output: &mut String, cell: &Cell) {
    output.push_str("\x1b[0");
    for (flag, code) in [
        (Flags::BOLD, "1"),
        (Flags::DIM, "2"),
        (Flags::ITALIC, "3"),
        (Flags::UNDERLINE, "4"),
        (Flags::DOUBLE_UNDERLINE, "4:2"),
        (Flags::UNDERCURL, "4:3"),
        (Flags::DOTTED_UNDERLINE, "4:4"),
        (Flags::DASHED_UNDERLINE, "4:5"),
        (Flags::INVERSE, "7"),
        (Flags::HIDDEN, "8"),
        (Flags::STRIKEOUT, "9"),
    ] {
        if cell.flags.contains(flag) {
            let _ = write!(output, ";{code}");
        }
    }
    for (color, prefix) in [(cell.fg, 38), (cell.bg, 48)] {
        match color {
            Color::Spec(rgb) => {
                let _ = write!(output, ";{prefix};2;{};{};{}", rgb.r, rgb.g, rgb.b);
            }
            Color::Indexed(index) => {
                let _ = write!(output, ";{prefix};5;{index}");
            }
            Color::Named(NamedColor::Foreground | NamedColor::Background) => {}
            Color::Named(color) => {
                let index = color as usize;
                if index < 8 {
                    let _ = write!(output, ";{}", prefix - 8 + index);
                } else if index < 16 {
                    let _ = write!(output, ";{}", prefix + 52 + index - 8);
                }
            }
        }
    }
    match cell.underline_color() {
        Some(Color::Spec(rgb)) => {
            let _ = write!(output, ";58;2;{};{};{}", rgb.r, rgb.g, rgb.b);
        }
        Some(Color::Indexed(index)) => {
            let _ = write!(output, ";58;5;{index}");
        }
        Some(Color::Named(color)) if (color as usize) < 256 => {
            let _ = write!(output, ";58;5;{}", color as usize);
        }
        _ => {}
    }
    output.push('m');
    if let Some(link) = cell.hyperlink() {
        let _ = write!(output, "\x1b]8;id={};{}\x1b\\", link.id(), link.uri());
    } else {
        output.push_str("\x1b]8;;\x1b\\");
    }
}

fn write_cursor(
    output: &mut String,
    cursor: &alacritty_terminal::grid::Cursor<Cell>,
    grid: &alacritty_terminal::Grid<Cell>,
    origin_top: i32,
) {
    let _ = write!(
        output,
        "\x1b[{};{}H",
        (cursor.point.line.0 - origin_top + 1).max(1),
        cursor.point.column.0 + 1
    );
    write_style(output, &cursor.template);
    if cursor.input_needs_wrap {
        let mut point = cursor.point;
        if grid[point].flags.contains(Flags::WIDE_CHAR_SPACER) {
            point.column.0 = point.column.0.saturating_sub(1);
        }
        let cell = &grid[point];
        let _ = write!(
            output,
            "\x1b[{};{}H",
            (point.line.0 - origin_top + 1).max(1),
            point.column.0 + 1
        );
        write_style(output, cell);
        output.push(cell.c);
        output.extend(cell.zerowidth().unwrap_or_default());
        write_style(output, &cursor.template);
    }
}

/// Locate a replay boundary without cutting a UTF-8 character or escape sequence.
#[derive(Default)]
pub(crate) struct ReplayBoundary {
    state: BoundaryState,
    pending: Vec<u8>,
    pending_len: u64,
    total: u64,
    sync_start: Option<u64>,
    scroll_region: Option<String>,
    before_sync_scroll_region: Option<String>,
}

pub(crate) struct AltScreenSwitch {
    /// Exclusive byte index of the completed mode sequence in this feed.
    pub end: usize,
    pub enter: bool,
    pub scroll_region: Option<String>,
}

#[derive(Default)]
enum BoundaryState {
    #[default]
    Ground,
    Escape,
    Csi,
    String {
        osc: bool,
        escape: bool,
    },
    Utf8(u8),
}

impl ReplayBoundary {
    pub(crate) fn feed(&mut self, bytes: &[u8]) -> Vec<AltScreenSwitch> {
        let mut switches = Vec::new();
        for (index, &byte) in bytes.iter().enumerate() {
            self.total += 1;
            self.pending_len += 1;
            if self.pending.len() < 64 {
                self.pending.push(byte);
            }
            self.state = match self.state {
                BoundaryState::Ground => match byte {
                    0x1b => BoundaryState::Escape,
                    0xc2..=0xdf => BoundaryState::Utf8(1),
                    0xe0..=0xef => BoundaryState::Utf8(2),
                    0xf0..=0xf4 => BoundaryState::Utf8(3),
                    _ => BoundaryState::Ground,
                },
                BoundaryState::Escape => match byte {
                    b'[' => BoundaryState::Csi,
                    b']' => BoundaryState::String {
                        osc: true,
                        escape: false,
                    },
                    b'P' | b'_' | b'^' | b'X' => BoundaryState::String {
                        osc: false,
                        escape: false,
                    },
                    0x20..=0x2f => BoundaryState::Escape,
                    b'c' => {
                        self.scroll_region = None;
                        BoundaryState::Ground
                    }
                    _ => BoundaryState::Ground,
                },
                BoundaryState::Csi if (0x40..=0x7e).contains(&byte) => {
                    if byte == b'r'
                        && self.pending.len() < 64
                        && self.pending[2..self.pending.len() - 1]
                            .iter()
                            .all(|byte| byte.is_ascii_digit() || *byte == b';')
                    {
                        self.scroll_region = String::from_utf8(self.pending.clone()).ok();
                    }
                    if self.pending == b"\x1b[?2026h" {
                        self.sync_start = Some(self.total - self.pending.len() as u64);
                        self.before_sync_scroll_region = self.scroll_region.clone();
                    }
                    if self.pending == b"\x1b[?2026l" {
                        self.sync_start = None;
                    }
                    if matches!(byte, b'h' | b'l')
                        && self.pending.starts_with(b"\x1b[?")
                        && self.pending[3..self.pending.len() - 1]
                            .split(|part| *part == b';')
                            .any(|number| matches!(number, b"47" | b"1047" | b"1049"))
                    {
                        switches.push(AltScreenSwitch {
                            end: index + 1,
                            enter: byte == b'h',
                            scroll_region: self.scroll_region.clone(),
                        });
                    }
                    BoundaryState::Ground
                }
                BoundaryState::Csi => BoundaryState::Csi,
                BoundaryState::String { osc, escape } => {
                    if (osc && byte == 7) || (escape && byte == b'\\') {
                        BoundaryState::Ground
                    } else {
                        BoundaryState::String {
                            osc,
                            escape: byte == 0x1b,
                        }
                    }
                }
                BoundaryState::Utf8(remaining) if remaining > 1 && byte & 0xc0 == 0x80 => {
                    BoundaryState::Utf8(remaining - 1)
                }
                BoundaryState::Utf8(_) => BoundaryState::Ground,
            };
            if matches!(self.state, BoundaryState::Ground) {
                self.pending.clear();
                self.pending_len = 0;
            }
        }
        switches
    }

    pub(crate) fn offset(&self, syncing: bool) -> u64 {
        if syncing {
            self.sync_start.unwrap_or(self.total)
        } else {
            self.total.saturating_sub(self.pending_len)
        }
    }

    pub(crate) fn trailing_bytes(&self, syncing: bool) -> u64 {
        self.total.saturating_sub(self.offset(syncing))
    }

    pub(crate) fn scroll_region(&self, syncing: bool) -> Option<&str> {
        if syncing {
            self.before_sync_scroll_region.as_deref()
        } else {
            self.scroll_region.as_deref()
        }
    }

    pub(crate) fn resized(&mut self) {
        self.scroll_region = None;
        self.before_sync_scroll_region = None;
    }
}

#[cfg(test)]
mod tests {
    use crate::{pty::RawOutput, terminal::TerminalOutput};

    fn append(screen: &TerminalOutput, raw: &RawOutput, bytes: &[u8]) {
        screen.feed_and_record(bytes, bytes, raw);
    }

    fn restore(screen: &TerminalOutput, raw: &RawOutput) -> TerminalOutput {
        let checkpoint = screen.checkpoint(raw).unwrap();
        let restored = TerminalOutput::from_replay(
            checkpoint.rows,
            checkpoint.columns,
            100,
            checkpoint.ansi.as_bytes(),
        );
        let suffix = raw.read(Some(checkpoint.offset), usize::MAX);
        assert_eq!(suffix.start_offset, checkpoint.offset);
        restored.feed_with_replies(&suffix.data);
        restored
    }

    #[test]
    fn checkpoint_retains_transcript_and_composer_after_animation_evicts_raw_output() {
        let screen = TerminalOutput::from_replay(6, 40, 100, b"");
        let raw = RawOutput::from_replay(256, b"");
        append(&screen, &raw, "old history\r\nmore history\r\nAnswer complete\r\n\x1b[48;2;52;56;64m\x1b[K\r\n› Ask Codex\x1b[K\r\n\x1b[K\x1b[0m\x1b[?2004h\x1b[?1004h".as_bytes());
        for _ in 0..100 {
            append(
                &screen,
                &raw,
                "\x1b[4;20H\x1b[38;2;100;110;120;48;2;52;56;64m⠁\x1b[0m\x1b[5;3H".as_bytes(),
            );
        }
        assert!(!String::from_utf8_lossy(&raw.snapshot()).contains("Answer complete"));
        let restored = restore(&screen, &raw);
        assert_eq!(
            screen.read_rows(0..usize::MAX),
            restored.read_rows(0..usize::MAX)
        );
        assert!(restored.is_bracketed_paste());
        assert!(restored.is_focus_reporting());
    }

    #[test]
    fn alternate_checkpoint_restores_static_tui_modes_and_underlying_primary_after_ring_wrap() {
        const RING_CAPACITY: usize = 8 * 1024 * 1024;
        let screen = TerminalOutput::from_replay(8, 40, 100, b"");
        let raw = RawOutput::from_replay(RING_CAPACITY, b"");
        append(&screen, &raw, b"shell history\r\nready> ");
        // A mode sequence can arrive across separate PTY reads.
        append(&screen, &raw, b"\x1b[?10");
        append(
            &screen,
            &raw,
            b"49h\x1b[?1002h\x1b[?1006h\x1b[?2004h\x1b[?1004h\x1b[?1h\x1b=",
        );
        append(
            &screen,
            &raw,
            b"\x1b[1;1HOpenCode\x1b[2;1H388,535 tokens / 39% / $22\x1b[3;1HStatic sidebar\x1b[4;1H\x1b[48;2;52;56;64m\x1b[40X\x1b[0m",
        );
        let diff = b"\x1b[?2026h\x1b[6;2H.\x1b[?2026l";
        let mut chunk = Vec::new();
        while chunk.len() < 64 * 1024 {
            chunk.extend_from_slice(diff);
        }
        let mut sent = 0;
        while sent <= RING_CAPACITY {
            append(&screen, &raw, &chunk);
            sent += chunk.len();
        }
        assert!(!String::from_utf8_lossy(&raw.snapshot()).contains("Static sidebar"));
        assert!(!String::from_utf8_lossy(&raw.snapshot()).contains("\x1b[?1049h"));

        let checkpoint = screen.checkpoint(&raw).unwrap();
        for expected in [
            "shell history",
            "\x1b[?1049h",
            "OpenCode",
            "388,535 tokens / 39% / $22",
            "Static sidebar",
            "\x1b[?1002h",
            "\x1b[?1006h",
            "\x1b[?2004h",
            "\x1b[?1004h",
            "\x1b[?1h",
            "\x1b=",
        ] {
            assert!(checkpoint.ansi.contains(expected), "missing {expected:?}");
        }
        let restored = restore(&screen, &raw);
        let actual = screen.read_rows(0..usize::MAX);
        let replayed = restored.read_rows(0..usize::MAX);
        assert_eq!(actual.text(), replayed.text());
        assert_eq!(actual.cursor, replayed.cursor);
        for (index, (left, right)) in actual.rows.iter().zip(&replayed.rows).enumerate() {
            assert_eq!(left, right, "row {index}");
        }
        assert!(restored.read_rows(0..usize::MAX).alternate_screen);
        assert!(restored.is_bracketed_paste());
        assert!(restored.is_focus_reporting());

        append(&screen, &raw, b"\x1b[?1049l\r\nreturned to shell");
        restored.feed_with_replies(b"\x1b[?1049l\r\nreturned to shell");
        assert_eq!(
            screen.read_rows(0..usize::MAX),
            restored.read_rows(0..usize::MAX)
        );
        assert!(!restored.read_rows(0..usize::MAX).alternate_screen);
        assert!(
            restored
                .read_rows(0..usize::MAX)
                .text()
                .contains("shell history")
        );
    }

    #[test]
    fn alternate_checkpoint_preserves_origin_mode_and_scroll_region() {
        let screen = TerminalOutput::from_replay(8, 20, 100, b"");
        let raw = RawOutput::from_replay(512, b"");
        append(&screen, &raw, b"\x1b[2;6r\x1b[?6h\x1b[2;2Hprimary");
        append(&screen, &raw, b"\x1b[?1049h\x1b[2;3Halt text");
        for _ in 0..100 {
            append(&screen, &raw, b"\x1b[3;4H.");
        }
        let restored = restore(&screen, &raw);
        assert_eq!(
            screen.read_rows(0..usize::MAX),
            restored.read_rows(0..usize::MAX)
        );
        append(&screen, &raw, b"\x1b[?1049l");
        restored.feed_with_replies(b"\x1b[?1049l");
        assert_eq!(
            screen.read_rows(0..usize::MAX),
            restored.read_rows(0..usize::MAX)
        );
    }

    #[test]
    fn alternate_checkpoint_preserves_primary_after_resize_while_tui_is_active() {
        let screen = TerminalOutput::from_replay(5, 20, 100, b"");
        let raw = RawOutput::from_replay(512, b"");
        append(&screen, &raw, b"a fairly long primary line\r\nready> ");
        append(&screen, &raw, b"\x1b[?1049h\x1b[1;1HStatic TUI");
        screen.lock().resize(6, 12);
        append(&screen, &raw, b"\x1b[2;2H.");
        let restored = restore(&screen, &raw);
        assert_eq!(
            screen.read_rows(0..usize::MAX),
            restored.read_rows(0..usize::MAX)
        );
        append(&screen, &raw, b"\x1b[?1049l");
        restored.feed_with_replies(b"\x1b[?1049l");
        assert_eq!(
            screen.read_rows(0..usize::MAX),
            restored.read_rows(0..usize::MAX)
        );
    }

    #[test]
    fn alternate_entry_inside_synchronized_update_keeps_primary() {
        let screen = TerminalOutput::from_replay(5, 20, 100, b"");
        let raw = RawOutput::from_replay(256, b"");
        append(&screen, &raw, b"shell before TUI");
        append(&screen, &raw, b"\x1b[?2026h\x1b[?1049h\x1b[1;1HTUI label\x1b[?2026l");
        for _ in 0..100 {
            append(&screen, &raw, b"\x1b[4;2H.");
        }
        let restored = restore(&screen, &raw);
        assert_eq!(screen.read_rows(0..usize::MAX), restored.read_rows(0..usize::MAX));
        append(&screen, &raw, b"\x1b[?1049l");
        restored.feed_with_replies(b"\x1b[?1049l");
        assert_eq!(screen.read_rows(0..usize::MAX), restored.read_rows(0..usize::MAX));
    }

    #[test]
    fn checkpoint_replays_partial_utf8_csi_and_synchronized_frames_once() {
        let updates = [
            "\x1b[3;2H\x1b[38;2;20;30;40m界e\u{301}",
            "\x1b[?2026h\x1b[3;2Hupdated\x1b[?2026l",
            "\x1b]11;#112233\x07\x1b[2;2Hdone",
        ];
        for update in updates {
            for split in 0..=update.len() {
                let screen = TerminalOutput::from_replay(5, 20, 100, b"");
                let raw = RawOutput::from_replay(8192, b"");
                append(&screen, &raw, b"before\r\n\x1b[1;31mhistory\x1b[0m");
                append(&screen, &raw, &update.as_bytes()[..split]);
                let restored = restore(&screen, &raw);
                append(&screen, &raw, &update.as_bytes()[split..]);
                restored.feed_with_replies(&update.as_bytes()[split..]);
                assert_eq!(
                    screen.read_rows(0..usize::MAX),
                    restored.read_rows(0..usize::MAX),
                    "split {split}: {update:?}"
                );
            }
        }
    }

    #[test]
    fn checkpoint_preserves_wrapped_scrollback_and_saved_cursor() {
        let screen = TerminalOutput::from_replay(3, 10, 100, b"");
        let raw = RawOutput::from_replay(8192, b"");
        append(
            &screen,
            &raw,
            "long wrapped output with 界\r\nnext\r\nlast\x1b7\x1b[1;3H\x1b[32m".as_bytes(),
        );
        let restored = restore(&screen, &raw);
        assert_eq!(
            screen.read_rows(0..usize::MAX),
            restored.read_rows(0..usize::MAX)
        );
        append(&screen, &raw, b"X\x1b8Y");
        restored.feed_with_replies(b"X\x1b8Y");
        assert_eq!(
            screen.read_rows(0..usize::MAX),
            restored.read_rows(0..usize::MAX)
        );
    }

    #[test]
    fn checkpoint_preserves_pending_wrap_and_scrolling_region() {
        for setup in [
            b"0123456789".as_slice(),
            b"0123456789 \x1b[3;2H",
            b"0123456789\x1b7\x1b[3;2H",
            b"\x1b[2;3r\x1b[3;10H!",
        ] {
            let screen = TerminalOutput::from_replay(4, 10, 100, b"");
            let raw = RawOutput::from_replay(8192, b"");
            append(&screen, &raw, setup);
            let restored = restore(&screen, &raw);
            append(&screen, &raw, b"next\r\nlast");
            restored.feed_with_replies(b"next\r\nlast");
            assert_eq!(
                screen.read_rows(0..usize::MAX),
                restored.read_rows(0..usize::MAX)
            );
            append(&screen, &raw, b"\x1b8Z");
            restored.feed_with_replies(b"\x1b8Z");
            assert_eq!(
                screen.read_rows(0..usize::MAX),
                restored.read_rows(0..usize::MAX)
            );
        }
    }

    #[test]
    fn checkpoint_retains_clickable_links_and_underline_colors() {
        let screen = TerminalOutput::from_replay(4, 40, 100, b"");
        let raw = RawOutput::from_replay(8192, b"");
        append(&screen, &raw, b"\x1b]8;id=source;file:///tmp/example.rs\x1b\\\x1b[4;58;2;20;30;40mView source\x1b]8;;\x1b\\\x1b[0m");
        let checkpoint = screen.checkpoint(&raw).unwrap();
        let restored = restore(&screen, &raw);
        let replay_raw = RawOutput::from_replay(8192, checkpoint.ansi.as_bytes());
        let next = restored.checkpoint(&replay_raw).unwrap();
        for ansi in [&checkpoint.ansi, &next.ansi] {
            assert!(ansi.contains("\x1b]8;id=source;file:///tmp/example.rs\x1b\\View source"));
            assert!(ansi.contains(";58;2;20;30;40m"));
        }
    }

    #[test]
    fn checkpoint_padding_does_not_add_lines_when_the_terminal_narrows() {
        let screen = TerminalOutput::from_replay(8, 100, 100, b"");
        let raw = RawOutput::from_replay(8192, b"");
        append(
            &screen,
            &raw,
            b"Completed answer\r\n\x1b[48;2;52;56;64m\x1b[K\r\ncomposer\x1b[K\r\n\x1b[K\x1b[0m",
        );
        let restored = restore(&screen, &raw);
        screen.lock().resize(10, 72);
        restored.lock().resize(10, 72);
        assert_eq!(
            screen.read_rows(0..usize::MAX),
            restored.read_rows(0..usize::MAX)
        );
    }
}
