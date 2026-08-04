//! Server-side terminal emulation and rendered-output queries.

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
    terminal: Term<VoidListener>,
    parser: ansi::Processor,
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
            scrollback_lines,
        }
    }

    /// Feed bytes read from the PTY into the terminal state machine.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.terminal, bytes);
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

    pub(crate) fn lock(&self) -> MutexGuard<'_, TerminalEmulator> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn feed(&self, bytes: &[u8]) {
        self.lock().feed(bytes);
    }

    pub fn read_rows(&self, range: Range<usize>) -> RenderedRows {
        self.lock().read_rows(range)
    }

    pub fn search_rendered(&self, needle: &str, max_matches: usize) -> Vec<RenderedSearchMatch> {
        self.lock().search_rendered(needle, max_matches)
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
}
