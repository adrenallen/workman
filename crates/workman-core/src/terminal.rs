//! Server-side terminal emulation and rendered-output queries.

use std::collections::VecDeque;
use std::fmt;
use std::ops::Range;
use std::sync::{Arc, Mutex, MutexGuard};

use alacritty_terminal::Term;
use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, LineLength};
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi;

/// Colors and flags retained for rendered terminal cells.
pub use alacritty_terminal::term::cell::Flags as CellFlags;
pub use alacritty_terminal::vte::ansi::{Color as CellColor, NamedColor, Rgb};

/// Default maximum number of off-screen rows retained per terminal.
pub const DEFAULT_SCROLLBACK_LINES: usize = 10_000;

/// Kitty's disambiguate-escape-codes progressive enhancement flag.
///
/// Workman intentionally advertises only this subset of the kitty keyboard protocol. The
/// frontend encodes the modified control keys used by interactive agent composers, while xterm
/// retains its existing behavior for ordinary text and keys outside that subset.
pub const KITTY_KEYBOARD_DISAMBIGUATE: u8 = 1;

const KEYBOARD_MODE_STACK_LIMIT: usize = 64;

/// Keyboard input protocol negotiated by the application currently running in a PTY.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TerminalKeyboardProtocol {
    pub kitty_flags: u8,
    pub modify_other_keys: u8,
}

#[derive(Debug, Default)]
struct KeyboardModeScreen {
    flags: u8,
    stack: VecDeque<u8>,
}

impl KeyboardModeScreen {
    fn set(&mut self, requested: u16, mode: u16) {
        let supported = (requested & u16::from(KITTY_KEYBOARD_DISAMBIGUATE)) as u8;
        match mode {
            1 => self.flags = supported,
            2 => self.flags |= supported,
            3 => self.flags &= !supported,
            _ => {}
        }
    }

    fn push(&mut self, requested: u16) {
        if self.stack.len() == KEYBOARD_MODE_STACK_LIMIT {
            self.stack.pop_front();
        }
        self.stack.push_back(self.flags);
        self.flags = (requested & u16::from(KITTY_KEYBOARD_DISAMBIGUATE)) as u8;
    }

    fn pop(&mut self, count: u16) {
        for _ in 0..count.max(1) {
            self.flags = self.stack.pop_back().unwrap_or_default();
        }
    }
}

#[derive(Debug, Default)]
enum CsiScanState {
    #[default]
    Ground,
    Escape,
    Csi(Vec<u8>),
}

/// Focused streaming parser for keyboard-protocol negotiation sequences that Alacritty does not
/// expose. It observes output without changing rendering and survives CSI sequences split across
/// arbitrary PTY read boundaries.
#[derive(Debug, Default)]
struct KeyboardProtocolTracker {
    main: KeyboardModeScreen,
    alternate: KeyboardModeScreen,
    alternate_screen: bool,
    modify_other_keys: u8,
    scan: CsiScanState,
    /// Plain `CSI 6 n` cursor probes seen but not yet answered. ConPTY emits one at
    /// session start and withholds child output until the hosting terminal replies.
    #[cfg(windows)]
    cursor_probes: usize,
}

impl KeyboardProtocolTracker {
    fn state(&self) -> TerminalKeyboardProtocol {
        TerminalKeyboardProtocol {
            kitty_flags: self.active_screen().flags,
            modify_other_keys: self.modify_other_keys,
        }
    }

    fn active_screen(&self) -> &KeyboardModeScreen {
        if self.alternate_screen {
            &self.alternate
        } else {
            &self.main
        }
    }

    fn active_screen_mut(&mut self) -> &mut KeyboardModeScreen {
        if self.alternate_screen {
            &mut self.alternate
        } else {
            &mut self.main
        }
    }

    fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut replies = Vec::new();
        for &byte in bytes {
            let state = std::mem::take(&mut self.scan);
            self.scan = match state {
                CsiScanState::Ground if byte == 0x1b => CsiScanState::Escape,
                CsiScanState::Ground if byte == 0x9b => CsiScanState::Csi(Vec::new()),
                CsiScanState::Ground => CsiScanState::Ground,
                CsiScanState::Escape if byte == b'[' => CsiScanState::Csi(Vec::new()),
                CsiScanState::Escape if byte == 0x1b => CsiScanState::Escape,
                CsiScanState::Escape => CsiScanState::Ground,
                CsiScanState::Csi(_parameters) if byte == 0x1b => CsiScanState::Escape,
                CsiScanState::Csi(parameters) if (0x40..=0x7e).contains(&byte) => {
                    self.handle_csi(&parameters, byte, &mut replies);
                    CsiScanState::Ground
                }
                CsiScanState::Csi(mut parameters)
                    if (0x20..=0x3f).contains(&byte) && parameters.len() < 64 =>
                {
                    parameters.push(byte);
                    CsiScanState::Csi(parameters)
                }
                CsiScanState::Csi(_) => CsiScanState::Ground,
            };
        }
        replies
    }

    fn handle_csi(&mut self, body: &[u8], final_byte: u8, replies: &mut Vec<Vec<u8>>) {
        let Some((&prefix, parameters)) = body.split_first() else {
            return;
        };
        let Some(parameters) = csi_parameters(parameters) else {
            return;
        };

        match (prefix, final_byte) {
            (b'=', b'u') => {
                let flags = parameter(&parameters, 0, 0);
                let mode = parameter(&parameters, 1, 1);
                self.active_screen_mut().set(flags, mode);
            }
            (b'>', b'u') => {
                let flags = parameter(&parameters, 0, 0);
                self.active_screen_mut().push(flags);
            }
            (b'<', b'u') => {
                let count = parameter(&parameters, 0, 1);
                self.active_screen_mut().pop(count);
            }
            (b'?', b'u')
                if parameters
                    .iter()
                    .all(|value| value.unwrap_or_default() == 0) =>
            {
                replies.push(format!("\x1b[?{}u", self.active_screen().flags).into_bytes());
            }
            (b'>', b'm') => self.set_modify_other_keys(&parameters),
            (b'?', b'm') if parameter(&parameters, 0, 0) == 4 => {
                replies.push(format!("\x1b[>4;{}m", self.modify_other_keys).into_bytes());
            }
            (b'>', b'n') if parameter(&parameters, 0, 2) == 4 => {
                self.modify_other_keys = 0;
            }
            #[cfg(windows)]
            (b'6', b'n') if parameters.iter().all(Option::is_none) => {
                self.cursor_probes += 1;
            }
            (b'?', b'h') | (b'?', b'l')
                if parameters
                    .iter()
                    .flatten()
                    .any(|mode| matches!(*mode, 47 | 1047 | 1049)) =>
            {
                self.alternate_screen = final_byte == b'h';
            }
            _ => {}
        }
    }

    fn set_modify_other_keys(&mut self, parameters: &[Option<u16>]) {
        if parameters.is_empty() || parameter(parameters, 0, 0) != 4 {
            self.modify_other_keys = 0;
            return;
        }
        self.modify_other_keys = match parameter(parameters, 1, 0) {
            1 => 1,
            2 => 2,
            _ => 0,
        };
    }
}

fn csi_parameters(bytes: &[u8]) -> Option<Vec<Option<u16>>> {
    bytes
        .split(|byte| *byte == b';')
        .map(|parameter| {
            if parameter.is_empty() {
                Some(None)
            } else if parameter.iter().all(u8::is_ascii_digit) {
                std::str::from_utf8(parameter).ok()?.parse().ok().map(Some)
            } else {
                None
            }
        })
        .collect()
}

fn parameter(parameters: &[Option<u16>], index: usize, default: u16) -> u16 {
    parameters
        .get(index)
        .and_then(|value| *value)
        .unwrap_or(default)
}

#[derive(Clone, Copy, Debug)]
struct EmulatorSize {
    rows: usize,
    columns: usize,
}

impl EmulatorSize {
    fn new(rows: u16, columns: u16) -> Self {
        Self {
            rows: usize::from(rows.max(1)),
            columns: usize::from(columns.max(1)),
        }
    }
}

impl Dimensions for EmulatorSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// One visible character and its terminal styling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedCell {
    /// Zero-based terminal column occupied by this character.
    pub column: usize,
    /// Character visible to the user. Hidden cells are represented as spaces.
    pub character: char,
    /// Combining characters attached to `character`.
    pub zero_width: Vec<char>,
    /// Cell width in terminal columns (normally one or two).
    pub width: usize,
    pub foreground: CellColor,
    pub background: CellColor,
    pub flags: CellFlags,
}

/// One physical terminal grid row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedRow {
    /// Zero-based row in the full retained buffer, oldest row first.
    pub index: usize,
    /// Alacritty grid line (`0` is the viewport top; history is negative).
    pub grid_line: i32,
    /// Plain visible text with trailing empty cells removed.
    pub text: String,
    /// Styled non-spacer cells contributing to `text`.
    pub cells: Vec<RenderedCell>,
    /// Whether this physical row wraps into the next row.
    pub wrapped: bool,
}

/// Cursor location in full-buffer row coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderedCursor {
    pub row: usize,
    pub column: usize,
    pub visible: bool,
}

/// A ranged snapshot of the terminal grid and scrollback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedRows {
    /// Inclusive first returned full-buffer row.
    pub start: usize,
    /// Exclusive end returned full-buffer row.
    pub end: usize,
    /// Total retained rows, including the viewport.
    pub total_rows: usize,
    /// First full-buffer row belonging to the visible viewport.
    pub viewport_start: usize,
    pub screen_rows: usize,
    pub columns: usize,
    pub alternate_screen: bool,
    pub cursor: RenderedCursor,
    pub rows: Vec<RenderedRow>,
}

impl RenderedRows {
    /// Join returned physical rows with newlines.
    pub fn text(&self) -> String {
        self.rows
            .iter()
            .map(|row| row.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A literal match in rendered, escape-sequence-free text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedSearchMatch {
    /// Full-buffer row containing the match.
    pub row: usize,
    /// UTF-8 byte range within `row_text`.
    pub byte_range: Range<usize>,
    pub row_text: String,
}

/// Stateful Alacritty parser and terminal grid for one process.
pub struct TerminalEmulator {
    // Deliberately rendering-only: xterm is the sole live authority for terminal query
    // replies (except the Windows-only ConPTY cursor probe in `feed_with_replies`).
    // A listener here would emit a second DA/DSR/color response into the same PTY.
    terminal: Term<VoidListener>,
    parser: ansi::Processor,
    keyboard_protocol: KeyboardProtocolTracker,
    scrollback_lines: usize,
}

impl TerminalEmulator {
    /// Create an empty terminal with bounded scrollback.
    pub fn new(rows: u16, columns: u16, scrollback_lines: usize) -> Self {
        let size = EmulatorSize::new(rows, columns);
        let config = Config {
            scrolling_history: scrollback_lines,
            ..Config::default()
        };
        Self {
            terminal: Term::new(config, &size, VoidListener),
            parser: ansi::Processor::new(),
            keyboard_protocol: KeyboardProtocolTracker::default(),
            scrollback_lines,
        }
    }

    /// Feed bytes read from the PTY into the terminal state machine.
    pub fn feed(&mut self, bytes: &[u8]) {
        let _ = self.feed_with_replies(bytes);
    }

    fn feed_with_replies(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let replies = self.keyboard_protocol.feed(bytes);
        #[cfg(windows)]
        let mut replies = replies;
        self.parser.advance(&mut self.terminal, bytes);
        // ConPTY withholds all child output until its startup `CSI 6 n` probe is
        // answered, and a headless daemon PTY has no live xterm frontend to answer
        // it, so on Windows the daemon is the hosting terminal and must reply.
        // Unix PTYs never stall on this query; xterm stays the sole authority there.
        #[cfg(windows)]
        {
            let cursor_probes = std::mem::take(&mut self.keyboard_protocol.cursor_probes);
            if cursor_probes > 0 {
                let point = self.terminal.grid().cursor.point;
                let report = format!("\x1b[{};{}R", point.line.0.max(0) + 1, point.column.0 + 1);
                for _ in 0..cursor_probes {
                    replies.push(report.clone().into_bytes());
                }
            }
        }
        replies
    }

    /// Resize the active and inactive grids, reflowing primary-screen content.
    pub fn resize(&mut self, rows: u16, columns: u16) {
        self.terminal.resize(EmulatorSize::new(rows, columns));
    }

    /// Read a clamped range of physical rows across scrollback and viewport.
    pub fn read_rows(&self, range: Range<usize>) -> RenderedRows {
        let history_rows = self.terminal.history_size();
        let screen_rows = self.terminal.screen_lines();
        let columns = self.terminal.columns();
        let total_rows = history_rows + screen_rows;
        let start = range.start.min(total_rows);
        let end = range.end.max(start).min(total_rows);
        let grid = self.terminal.grid();

        let rows = (start..end)
            .map(|index| {
                let grid_line = index as i32 - history_rows as i32;
                render_row(index, grid_line, columns, &grid[Line(grid_line)])
            })
            .collect();

        let cursor_point = grid.cursor.point;
        let cursor_row = (history_rows as i32 + cursor_point.line.0)
            .clamp(0, total_rows.saturating_sub(1) as i32) as usize;

        RenderedRows {
            start,
            end,
            total_rows,
            viewport_start: history_rows,
            screen_rows,
            columns,
            alternate_screen: self.terminal.mode().contains(TermMode::ALT_SCREEN),
            cursor: RenderedCursor {
                row: cursor_row,
                column: cursor_point.column.0,
                visible: self.terminal.mode().contains(TermMode::SHOW_CURSOR),
            },
            rows,
        }
    }

    /// Search all retained rendered rows for a literal string.
    pub fn search_rendered(&self, needle: &str, max_matches: usize) -> Vec<RenderedSearchMatch> {
        if needle.is_empty() || max_matches == 0 {
            return Vec::new();
        }

        let mut matches = Vec::new();
        let history_rows = self.terminal.history_size();
        let total_rows = history_rows + self.terminal.screen_lines();
        let columns = self.terminal.columns();
        let grid = self.terminal.grid();
        for index in 0..total_rows {
            let grid_line = index as i32 - history_rows as i32;
            let row = render_row(index, grid_line, columns, &grid[Line(grid_line)]);
            for (start, _) in row.text.match_indices(needle) {
                matches.push(RenderedSearchMatch {
                    row: row.index,
                    byte_range: start..start + needle.len(),
                    row_text: row.text.clone(),
                });
                if matches.len() == max_matches {
                    return matches;
                }
            }
        }
        matches
    }

    pub fn screen_rows(&self) -> usize {
        self.terminal.screen_lines()
    }

    pub fn columns(&self) -> usize {
        self.terminal.columns()
    }

    pub fn history_rows(&self) -> usize {
        self.terminal.history_size()
    }

    pub fn scrollback_limit(&self) -> usize {
        self.scrollback_lines
    }

    pub fn is_alternate_screen(&self) -> bool {
        self.terminal.mode().contains(TermMode::ALT_SCREEN)
    }

    /// Whether the application has enabled DEC private focus reporting (mode 1004).
    pub fn is_focus_reporting(&self) -> bool {
        self.terminal.mode().contains(TermMode::FOCUS_IN_OUT)
    }

    pub fn keyboard_protocol(&self) -> TerminalKeyboardProtocol {
        self.keyboard_protocol.state()
    }
}

impl fmt::Debug for TerminalEmulator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TerminalEmulator")
            .field("screen_rows", &self.screen_rows())
            .field("columns", &self.columns())
            .field("history_rows", &self.history_rows())
            .field("scrollback_limit", &self.scrollback_lines)
            .field("alternate_screen", &self.is_alternate_screen())
            .field("focus_reporting", &self.is_focus_reporting())
            .field("keyboard_protocol", &self.keyboard_protocol())
            .finish_non_exhaustive()
    }
}

/// Thread-safe query handle for a process's terminal emulator.
#[derive(Clone)]
pub struct TerminalOutput {
    inner: Arc<Mutex<TerminalEmulator>>,
}

impl TerminalOutput {
    pub(crate) fn new(rows: u16, columns: u16, scrollback_lines: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TerminalEmulator::new(
                rows,
                columns,
                scrollback_lines,
            ))),
        }
    }

    /// Rebuild server-rendered terminal state by replaying retained raw bytes.
    pub fn from_replay(rows: u16, columns: u16, scrollback_lines: usize, bytes: &[u8]) -> Self {
        let output = Self::new(rows, columns, scrollback_lines);
        output.feed_and_read_viewport(bytes);
        output
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, TerminalEmulator> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn feed_and_read_viewport(&self, bytes: &[u8]) -> RenderedRows {
        self.feed_and_read_viewport_with_replies(bytes).0
    }

    pub(crate) fn feed_and_read_viewport_with_replies(
        &self,
        bytes: &[u8],
    ) -> (RenderedRows, Vec<Vec<u8>>) {
        let mut terminal = self.lock();
        let replies = terminal.feed_with_replies(bytes);
        let viewport_start = terminal.history_rows();
        (terminal.read_rows(viewport_start..usize::MAX), replies)
    }

    pub(crate) fn feed_with_replies(&self, bytes: &[u8]) -> Vec<Vec<u8>> {
        self.lock().feed_with_replies(bytes)
    }

    pub(crate) fn read_viewport(&self) -> RenderedRows {
        let terminal = self.lock();
        let viewport_start = terminal.history_rows();
        terminal.read_rows(viewport_start..usize::MAX)
    }

    pub fn read_rows(&self, range: Range<usize>) -> RenderedRows {
        self.lock().read_rows(range)
    }

    pub fn search_rendered(&self, needle: &str, max_matches: usize) -> Vec<RenderedSearchMatch> {
        self.lock().search_rendered(needle, max_matches)
    }

    /// Clear retained scrollback and the visible grid without detaching the PTY reader.
    pub fn clear(&self) {
        let mut terminal = self.lock();
        let rows = u16::try_from(terminal.screen_rows()).unwrap_or(u16::MAX);
        let columns = u16::try_from(terminal.columns()).unwrap_or(u16::MAX);
        let scrollback_lines = terminal.scrollback_limit();
        *terminal = TerminalEmulator::new(rows, columns, scrollback_lines);
    }

    pub fn screen_rows(&self) -> usize {
        self.lock().screen_rows()
    }

    pub fn columns(&self) -> usize {
        self.lock().columns()
    }

    pub fn history_rows(&self) -> usize {
        self.lock().history_rows()
    }

    /// Whether the latest PTY output enabled DEC private focus reporting (mode 1004).
    pub fn is_focus_reporting(&self) -> bool {
        self.lock().is_focus_reporting()
    }

    pub fn keyboard_protocol(&self) -> TerminalKeyboardProtocol {
        self.lock().keyboard_protocol()
    }
}

impl fmt::Debug for TerminalOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.lock().fmt(formatter)
    }
}

fn render_row(
    index: usize,
    grid_line: i32,
    columns: usize,
    row: &alacritty_terminal::grid::Row<Cell>,
) -> RenderedRow {
    let occupied = row.line_length().0.min(columns);
    let wrapped = columns > 0 && row[Column(columns - 1)].flags.contains(CellFlags::WRAPLINE);
    let mut text = String::new();
    let mut cells = Vec::with_capacity(occupied);

    for column in 0..occupied {
        let cell = &row[Column(column)];
        if cell
            .flags
            .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }

        let hidden = cell.flags.contains(CellFlags::HIDDEN);
        let character = if hidden { ' ' } else { cell.c };
        let zero_width = if hidden {
            Vec::new()
        } else {
            cell.zerowidth().unwrap_or_default().to_vec()
        };
        text.push(character);
        text.extend(zero_width.iter());
        cells.push(RenderedCell {
            column,
            character,
            zero_width,
            width: if cell.flags.contains(CellFlags::WIDE_CHAR) {
                2
            } else {
                1
            },
            foreground: cell.fg,
            background: cell.bg,
            flags: cell.flags,
        });
    }

    RenderedRow {
        index,
        grid_line,
        text,
        cells,
        wrapped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollback_is_bounded_and_ranges_are_clamped() {
        let mut emulator = TerminalEmulator::new(2, 12, 2);
        emulator.feed(b"one\r\ntwo\r\nthree\r\nfour\r\nfive");

        let snapshot = emulator.read_rows(0..usize::MAX);
        assert_eq!(snapshot.total_rows, 4);
        assert_eq!(snapshot.viewport_start, 2);
        assert_eq!(snapshot.text(), "two\nthree\nfour\nfive");

        let tail = emulator.read_rows(2..99);
        assert_eq!(tail.start, 2);
        assert_eq!(tail.end, 4);
        assert_eq!(tail.text(), "four\nfive");
    }

    #[test]
    fn rendered_search_ignores_escape_sequences() {
        let mut emulator = TerminalEmulator::new(3, 20, 4);
        emulator.feed(b"plain \x1b[31mred\x1b[0m text\r\nred again");

        let matches = emulator.search_rendered("red", 10);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].row_text, "plain red text");
        assert_eq!(matches[0].byte_range, 6..9);
        assert_eq!(matches[1].row_text, "red again");
    }

    #[test]
    fn focus_reporting_tracks_dec_private_mode_1004() {
        let mut emulator = TerminalEmulator::new(3, 20, 4);
        assert!(!emulator.is_focus_reporting());

        emulator.feed(b"\x1b[?1004h");
        assert!(emulator.is_focus_reporting());

        emulator.feed(b"\x1b[?1004l");
        assert!(!emulator.is_focus_reporting());
    }

    #[test]
    fn kitty_keyboard_stack_is_streaming_and_screen_local() {
        let mut emulator = TerminalEmulator::new(3, 20, 4);
        assert_eq!(
            emulator.keyboard_protocol(),
            TerminalKeyboardProtocol::default()
        );

        emulator.feed(b"\x1b[>1");
        emulator.feed(b"u");
        assert_eq!(emulator.keyboard_protocol().kitty_flags, 1);

        emulator.feed(b"\x1b[?1049h");
        assert_eq!(emulator.keyboard_protocol().kitty_flags, 0);
        emulator.feed(b"\x1b[>1u");
        assert_eq!(emulator.keyboard_protocol().kitty_flags, 1);
        emulator.feed(b"\x1b[<u");
        assert_eq!(emulator.keyboard_protocol().kitty_flags, 0);

        emulator.feed(b"\x1b[?1049l");
        assert_eq!(emulator.keyboard_protocol().kitty_flags, 1);
        emulator.feed(b"\x1b[<u");
        assert_eq!(emulator.keyboard_protocol().kitty_flags, 0);
    }

    #[test]
    fn kitty_keyboard_queries_report_only_the_supported_flag() {
        let mut emulator = TerminalEmulator::new(3, 20, 4);
        let replies = emulator.feed_with_replies(b"\x1b[=31;1u\x1b[?u");
        assert_eq!(emulator.keyboard_protocol().kitty_flags, 1);
        assert_eq!(replies, vec![b"\x1b[?1u".to_vec()]);

        emulator.feed(b"\x1b[=1;3u");
        assert_eq!(emulator.keyboard_protocol().kitty_flags, 0);
        let replies = emulator.feed_with_replies(b"\x1b[?u");
        assert_eq!(replies, vec![b"\x1b[?0u".to_vec()]);
    }

    #[test]
    fn modify_other_keys_tracks_supported_levels_and_answers_queries() {
        let mut emulator = TerminalEmulator::new(3, 20, 4);
        emulator.feed(b"\x1b[>4;2m");
        assert_eq!(emulator.keyboard_protocol().modify_other_keys, 2);
        let replies = emulator.feed_with_replies(b"\x1b[?4m");
        assert_eq!(replies, vec![b"\x1b[>4;2m".to_vec()]);

        emulator.feed(b"\x1b[>4n");
        assert_eq!(emulator.keyboard_protocol().modify_other_keys, 0);
        emulator.feed(b"\x1b[>4;3m");
        assert_eq!(emulator.keyboard_protocol().modify_other_keys, 0);
    }

    #[cfg(windows)]
    #[test]
    fn conpty_cursor_probe_is_answered_from_the_rendered_cursor() {
        let mut emulator = TerminalEmulator::new(3, 20, 4);
        let replies = emulator.feed_with_replies(b"ok\x1b[6n");
        assert_eq!(replies, vec![b"\x1b[1;3R".to_vec()]);

        // A probe split across PTY reads still produces exactly one report.
        assert!(emulator.feed_with_replies(b"\x1b[6").is_empty());
        let replies = emulator.feed_with_replies(b"n");
        assert_eq!(replies, vec![b"\x1b[1;3R".to_vec()]);
    }

    #[cfg(not(windows))]
    #[test]
    fn cursor_probes_stay_unanswered_where_the_live_frontend_replies() {
        let mut emulator = TerminalEmulator::new(3, 20, 4);
        assert!(emulator.feed_with_replies(b"ok\x1b[6n").is_empty());
    }

    #[test]
    fn terminal_output_replay_restores_keyboard_protocol_without_live_replies() {
        let output = TerminalOutput::from_replay(3, 20, 4, b"\x1b[>1u\x1b[>4;2m\x1b[?u\x1b[?4m");
        assert_eq!(
            output.keyboard_protocol(),
            TerminalKeyboardProtocol {
                kitty_flags: 1,
                modify_other_keys: 2,
            }
        );
    }

    #[test]
    fn terminal_output_clear_keeps_the_shared_handle_live() {
        let output = TerminalOutput::new(3, 20, 4);
        output.feed_and_read_viewport(b"before clear");
        assert_eq!(output.search_rendered("before", 1).len(), 1);

        output.clear();
        assert!(output.search_rendered("before", 1).is_empty());
        assert_eq!(output.screen_rows(), 3);
        assert_eq!(output.columns(), 20);

        output.feed_and_read_viewport(b"after clear");
        assert_eq!(output.search_rendered("after", 1).len(), 1);
    }
}
