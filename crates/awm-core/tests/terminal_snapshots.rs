use std::fmt::Write;

use awm_core::terminal::{CellColor, CellFlags, NamedColor, RenderedRows, TerminalEmulator};

fn decode_recording(recording: &str) -> Vec<u8> {
    recording
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default())
        .flat_map(str::split_whitespace)
        .map(|byte| u8::from_str_radix(byte, 16).expect("hex byte in recording"))
        .collect()
}

fn color_name(color: CellColor) -> String {
    match color {
        CellColor::Named(color) => format!("{color:?}"),
        CellColor::Indexed(index) => format!("index-{index}"),
        CellColor::Spec(rgb) => format!("#{:02x}{:02x}{:02x}", rgb.r, rgb.g, rgb.b),
    }
}

fn snapshot(snapshot: &RenderedRows) -> String {
    let mut output = format!(
        "range={}..{} total={} viewport={} screen={} cols={} alt={} cursor={}:{}:{}\n",
        snapshot.start,
        snapshot.end,
        snapshot.total_rows,
        snapshot.viewport_start,
        snapshot.screen_rows,
        snapshot.columns,
        snapshot.alternate_screen,
        snapshot.cursor.row,
        snapshot.cursor.column,
        snapshot.cursor.visible,
    );

    for row in &snapshot.rows {
        writeln!(
            output,
            "{:03}{}|{}|",
            row.index,
            if row.wrapped { ">" } else { " " },
            row.text
        )
        .unwrap();

        let mut run_start = None;
        let mut previous_column = 0;
        let mut previous_style = None;
        for cell in &row.cells {
            let style = (cell.foreground, cell.background, cell.flags);
            let is_default = style
                == (
                    CellColor::Named(NamedColor::Foreground),
                    CellColor::Named(NamedColor::Background),
                    CellFlags::empty(),
                );
            if is_default {
                if let (Some(start), Some(style)) = (run_start.take(), previous_style.take()) {
                    write_style_run(&mut output, row.index, start, previous_column, style);
                }
                continue;
            }

            if previous_style == Some(style) && previous_column + 1 == cell.column {
                previous_column = cell.column;
            } else {
                if let (Some(start), Some(style)) = (run_start.take(), previous_style.take()) {
                    write_style_run(&mut output, row.index, start, previous_column, style);
                }
                run_start = Some(cell.column);
                previous_column = cell.column;
                previous_style = Some(style);
            }
        }
        if let (Some(start), Some(style)) = (run_start, previous_style) {
            write_style_run(&mut output, row.index, start, previous_column, style);
        }
    }

    output
}

fn write_style_run(
    output: &mut String,
    row: usize,
    start: usize,
    end: usize,
    style: (CellColor, CellColor, CellFlags),
) {
    writeln!(
        output,
        "    style {row}:{start}-{end} fg={} bg={} flags={:?}",
        color_name(style.0),
        color_name(style.1),
        style.2,
    )
    .unwrap();
}

#[test]
fn recorded_vim_alt_screen_and_colors_match_snapshot() {
    let recording = decode_recording(include_str!("fixtures/terminal/vim.recording.hex"));
    let mut emulator = TerminalEmulator::new(6, 24, 8);
    emulator.feed(&recording);

    let rendered = emulator.read_rows(0..usize::MAX);
    assert_eq!(
        snapshot(&rendered),
        include_str!("snapshots/vim-alt-screen.txt")
    );
    assert!(rendered.alternate_screen);
    assert_eq!(
        rendered.rows[0].cells[0].foreground,
        CellColor::Spec(awm_core::terminal::Rgb {
            r: 80,
            g: 250,
            b: 120,
        })
    );
    assert!(rendered.rows[5].cells[0].flags.contains(CellFlags::INVERSE));

    emulator.feed(b"\x1b[?1049l");
    let restored = emulator.read_rows(0..usize::MAX);
    assert!(!restored.alternate_screen);
    assert_eq!(restored.rows[0].text, "shell before vim");
}

#[test]
fn recorded_stream_reflows_across_resize() {
    let before = decode_recording(include_str!(
        "fixtures/terminal/resize-before.recording.hex"
    ));
    let after = decode_recording(include_str!("fixtures/terminal/resize-after.recording.hex"));
    let mut emulator = TerminalEmulator::new(3, 10, 3);
    emulator.feed(&before);
    emulator.resize(4, 5);
    emulator.feed(&after);

    let rendered = emulator.read_rows(0..usize::MAX);
    assert_eq!(
        snapshot(&rendered),
        include_str!("snapshots/resize-reflow.txt")
    );
    assert!(rendered.total_rows <= rendered.screen_rows + 3);
}
