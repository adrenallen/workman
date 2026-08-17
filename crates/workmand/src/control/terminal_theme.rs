use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::Serialize;

const PROFILE_HOME_ENV: &str = "WORKMAN_TERMINAL_PROFILE_HOME";
const COLOR_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "brightBlack",
    "brightRed",
    "brightGreen",
    "brightYellow",
    "brightBlue",
    "brightMagenta",
    "brightCyan",
    "brightWhite",
];

#[derive(Clone, Debug, Serialize)]
pub(super) struct TerminalThemeImport {
    imported: bool,
    source: Option<String>,
    profile: Option<String>,
    palette: Option<TerminalPalette>,
    #[serde(rename = "terminalStyle")]
    terminal_style: Option<TerminalProfileStyle>,
    message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TerminalProfileStyle {
    font_family: Option<String>,
    font_size: Option<f64>,
    line_height: Option<f64>,
    letter_spacing: Option<i64>,
    cursor_style: Option<String>,
    cursor_blink: Option<bool>,
    draw_bold_text_in_bright_colors: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TerminalPalette {
    background: String,
    foreground: String,
    cursor: String,
    selection: String,
    black: String,
    red: String,
    green: String,
    yellow: String,
    blue: String,
    magenta: String,
    cyan: String,
    white: String,
    bright_black: String,
    bright_red: String,
    bright_green: String,
    bright_yellow: String,
    bright_blue: String,
    bright_magenta: String,
    bright_cyan: String,
    bright_white: String,
}

#[derive(Clone, Debug)]
struct Candidate {
    source: &'static str,
    profile: String,
    colors: BTreeMap<String, String>,
    terminal_style: Option<TerminalProfileStyle>,
}

pub(super) fn import_terminal_theme() -> TerminalThemeImport {
    let home = std::env::var_os(PROFILE_HOME_ENV)
        .map(PathBuf::from)
        .or_else(dirs::home_dir);
    let Some(home) = home else {
        return not_found("Could not determine your home directory.");
    };
    import_terminal_theme_from(&home)
}

fn import_terminal_theme_from(home: &Path) -> TerminalThemeImport {
    let mut attempted = Vec::new();
    // Workman is a macOS desktop app and should initially match the native Terminal.app profile.
    // iTerm remains the next supported source and every source remains available to manual import.
    let sources: [fn(&Path) -> Option<Candidate>; 6] = [
        import_terminal_app,
        import_iterm2,
        import_ghostty,
        import_kitty,
        import_alacritty,
        import_wezterm,
    ];
    for importer in sources {
        if let Some(candidate) = importer(home) {
            let palette = palette_from_colors(&candidate.colors);
            return TerminalThemeImport {
                imported: true,
                source: Some(candidate.source.to_owned()),
                profile: Some(candidate.profile.clone()),
                palette: Some(palette),
                terminal_style: candidate.terminal_style,
                message: format!(
                    "Imported {} profile ‘{}’.",
                    candidate.source, candidate.profile
                ),
            };
        }
        attempted.push(match attempted.len() {
            0 => "Terminal.app",
            1 => "iTerm2",
            2 => "Ghostty",
            3 => "kitty",
            4 => "Alacritty",
            _ => "WezTerm",
        });
    }
    not_found(&format!(
        "No readable terminal profile was found (checked {}).",
        attempted.join(", ")
    ))
}

fn not_found(message: &str) -> TerminalThemeImport {
    TerminalThemeImport {
        imported: false,
        source: None,
        profile: None,
        palette: None,
        terminal_style: None,
        message: message.to_owned(),
    }
}

fn import_iterm2(home: &Path) -> Option<Candidate> {
    let path = home.join("Library/Preferences/com.googlecode.iterm2.plist");
    if !path.is_file() {
        return None;
    }
    let default_guid = plutil_raw_file(&path, "Default Bookmark Guid");
    let mut selected = None;
    for index in 0..128 {
        let base = format!("New Bookmarks.{index}");
        let Some(guid) = plutil_raw_file(&path, &format!("{base}.Guid")) else {
            break;
        };
        if selected.is_none() || default_guid.as_deref() == Some(guid.as_str()) {
            selected = Some((index, guid));
        }
        if default_guid.as_deref() == selected.as_ref().map(|(_, guid)| guid.as_str()) {
            break;
        }
    }
    let (index, _) = selected?;
    let base = format!("New Bookmarks.{index}");
    let profile =
        plutil_raw_file(&path, &format!("{base}.Name")).unwrap_or_else(|| "Default".to_owned());
    let mut colors = BTreeMap::new();
    for (target, source) in [
        ("background", "Background Color"),
        ("foreground", "Foreground Color"),
        ("cursor", "Cursor Color"),
        ("selection", "Selection Color"),
    ] {
        if let Some(color) = iterm_color(&path, &format!("{base}.{source}")) {
            colors.insert(target.to_owned(), color);
        }
    }
    for (index, name) in COLOR_NAMES.iter().enumerate() {
        if let Some(color) = iterm_color(&path, &format!("{base}.Ansi {index} Color")) {
            colors.insert((*name).to_owned(), color);
        }
    }
    let mut candidate = candidate("iTerm2", profile, colors)?;
    candidate.terminal_style = Some(iterm_style(&path, &base));
    Some(candidate)
}

fn iterm_style(path: &Path, base: &str) -> TerminalProfileStyle {
    let font = plutil_raw_file(path, &format!("{base}.Normal Font"))
        .and_then(|font| parse_named_font(&font));
    let font_size = font.as_ref().map(|(_, size)| *size);
    let horizontal_spacing = plutil_number_file(path, &format!("{base}.Horizontal Spacing"));
    TerminalProfileStyle {
        font_family: font.map(|(family, _)| family),
        font_size,
        line_height: plutil_number_file(path, &format!("{base}.Vertical Spacing"))
            .map(normalize_line_height),
        letter_spacing: horizontal_spacing
            .zip(font_size)
            .map(|(ratio, size)| ((ratio - 1.0) * size).round() as i64),
        cursor_style: plutil_raw_file(path, &format!("{base}.Cursor Type")).and_then(|value| {
            match value.as_str() {
                "0" => Some("block".to_owned()),
                "1" => Some("bar".to_owned()),
                "2" => Some("underline".to_owned()),
                _ => None,
            }
        }),
        cursor_blink: plutil_bool_file(path, &format!("{base}.Blinking Cursor")),
        draw_bold_text_in_bright_colors: plutil_bool_file(path, &format!("{base}.Use Bright Bold")),
    }
}

fn iterm_color(path: &Path, key: &str) -> Option<String> {
    let red = plutil_raw_file(path, &format!("{key}.Red Component"))?
        .parse()
        .ok()?;
    let green = plutil_raw_file(path, &format!("{key}.Green Component"))?
        .parse()
        .ok()?;
    let blue = plutil_raw_file(path, &format!("{key}.Blue Component"))?
        .parse()
        .ok()?;
    Some(rgb_hex(red, green, blue))
}

fn import_terminal_app(home: &Path) -> Option<Candidate> {
    let path = home.join("Library/Preferences/com.apple.Terminal.plist");
    if !path.is_file() {
        return None;
    }
    let profile = plutil_raw_file(&path, "Default Window Settings")
        .or_else(|| plutil_raw_file(&path, "Startup Window Settings"))?;
    let base = format!("Window Settings.{profile}");
    let mut colors = BTreeMap::new();
    for (target, source) in [
        ("background", "BackgroundColor"),
        ("foreground", "TextColor"),
        ("cursor", "CursorColor"),
        ("selection", "SelectionColor"),
    ] {
        if let Some(color) = terminal_app_color(&path, &format!("{base}.{source}")) {
            colors.insert(target.to_owned(), color);
        }
    }
    let terminal_names = [
        "ANSIBlackColor",
        "ANSIRedColor",
        "ANSIGreenColor",
        "ANSIYellowColor",
        "ANSIBlueColor",
        "ANSIMagentaColor",
        "ANSICyanColor",
        "ANSIWhiteColor",
        "ANSIBrightBlackColor",
        "ANSIBrightRedColor",
        "ANSIBrightGreenColor",
        "ANSIBrightYellowColor",
        "ANSIBrightBlueColor",
        "ANSIBrightMagentaColor",
        "ANSIBrightCyanColor",
        "ANSIBrightWhiteColor",
    ];
    for (target, source) in COLOR_NAMES.iter().zip(terminal_names) {
        if let Some(color) = terminal_app_color(&path, &format!("{base}.{source}")) {
            colors.insert((*target).to_owned(), color);
        }
    }
    let mut candidate = candidate("Terminal.app", profile, colors)?;
    candidate.terminal_style = Some(terminal_app_style(&path, &base));
    Some(candidate)
}

fn terminal_app_style(path: &Path, base: &str) -> TerminalProfileStyle {
    let font = plutil_raw_file(path, &format!("{base}.Font"))
        .and_then(|value| BASE64.decode(value).ok())
        .and_then(|archive| terminal_app_font(&archive));
    let font_size = font.as_ref().map(|(_, size)| *size);
    let width_spacing = plutil_number_file(path, &format!("{base}.FontWidthSpacing"));
    TerminalProfileStyle {
        font_family: font.map(|(family, _)| family),
        font_size,
        line_height: plutil_number_file(path, &format!("{base}.FontHeightSpacing"))
            .map(normalize_line_height),
        letter_spacing: width_spacing
            .zip(font_size)
            .map(|(ratio, size)| ((ratio - 1.0) * size).round() as i64),
        // Terminal.app stores 0=block, 1=underline, and 2=vertical bar. Missing keys mean its
        // native block/non-blinking defaults, which are also xterm's defaults.
        cursor_style: plutil_raw_file(path, &format!("{base}.CursorType")).and_then(|value| {
            match value.as_str() {
                "0" => Some("block".to_owned()),
                "1" => Some("underline".to_owned()),
                "2" => Some("bar".to_owned()),
                _ => None,
            }
        }),
        cursor_blink: plutil_bool_file(path, &format!("{base}.CursorBlink")),
        // Terminal.app leaves this key absent for its disabled default. xterm defaults it on,
        // so report the native default explicitly instead of silently brightening bold ANSI text.
        draw_bold_text_in_bright_colors: Some(
            plutil_bool_file(path, &format!("{base}.UseBrightBold")).unwrap_or(false),
        ),
    }
}

fn terminal_app_font(archive: &[u8]) -> Option<(String, f64)> {
    let archive = plist::Value::from_reader(std::io::Cursor::new(archive)).ok()?;
    let objects = archive.as_dictionary()?.get("$objects")?.as_array()?;
    let font_size = objects
        .iter()
        .find_map(|object| object.as_dictionary()?.get("NSSize")?.as_real())?;
    let family = objects.iter().find_map(|object| {
        let value = object.as_string()?;
        (value != "$null" && value != "NSFont" && value != "NSObject").then(|| value.to_owned())
    })?;
    Some((family, font_size))
}

fn parse_named_font(value: &str) -> Option<(String, f64)> {
    let (family, size) = value.rsplit_once(' ')?;
    let size = size.parse::<f64>().ok()?;
    (!family.trim().is_empty()).then(|| (family.trim().to_owned(), size))
}

fn normalize_line_height(value: f64) -> f64 {
    value.clamp(1.0, 3.0)
}

fn terminal_app_color(path: &Path, key: &str) -> Option<String> {
    let archived = BASE64.decode(plutil_raw_file(path, key)?).ok()?;
    for object_index in 1..8 {
        let base = format!("$objects.{object_index}");
        if let Some(data) = plutil_raw_stdin(&archived, &format!("{base}.NSRGB"))
            .and_then(|value| BASE64.decode(value).ok())
        {
            let values = String::from_utf8_lossy(&data)
                .split_whitespace()
                .filter_map(|value| value.trim_end_matches('\0').parse::<f64>().ok())
                .collect::<Vec<_>>();
            if values.len() >= 3 {
                return Some(rgb_hex(values[0], values[1], values[2]));
            }
        }
        if let Some(data) = plutil_raw_stdin(&archived, &format!("{base}.NSWhite"))
            .and_then(|value| BASE64.decode(value).ok())
        {
            let white = String::from_utf8_lossy(&data)
                .split_whitespace()
                .next()
                .and_then(|value| value.trim_end_matches('\0').parse::<f64>().ok());
            if let Some(white) = white {
                return Some(rgb_hex(white, white, white));
            }
        }
    }
    None
}

fn import_ghostty(home: &Path) -> Option<Candidate> {
    import_plain_candidates(
        home,
        "Ghostty",
        &[
            ".config/ghostty/config",
            "Library/Application Support/com.mitchellh.ghostty/config",
        ],
        PlainFlavor::Ghostty,
    )
}

fn import_kitty(home: &Path) -> Option<Candidate> {
    import_plain_candidates(
        home,
        "kitty",
        &[".config/kitty/kitty.conf"],
        PlainFlavor::Kitty,
    )
}

fn import_alacritty(home: &Path) -> Option<Candidate> {
    import_plain_candidates(
        home,
        "Alacritty",
        &[
            ".config/alacritty/alacritty.toml",
            ".config/alacritty/alacritty.yml",
            ".config/alacritty/alacritty.yaml",
            ".alacritty.yml",
        ],
        PlainFlavor::Alacritty,
    )
}

fn import_wezterm(home: &Path) -> Option<Candidate> {
    import_plain_candidates(
        home,
        "WezTerm",
        &[".wezterm.lua", ".config/wezterm/wezterm.lua"],
        PlainFlavor::WezTerm,
    )
}

#[derive(Clone, Copy)]
enum PlainFlavor {
    Ghostty,
    Kitty,
    Alacritty,
    WezTerm,
}

fn import_plain_candidates(
    home: &Path,
    source: &'static str,
    relative_paths: &[&str],
    flavor: PlainFlavor,
) -> Option<Candidate> {
    relative_paths.iter().find_map(|relative| {
        let path = home.join(relative);
        let contents = fs::read_to_string(path).ok()?;
        let colors = parse_plain(&contents, flavor);
        candidate(source, format!("~/{relative}"), colors)
    })
}

fn parse_plain(contents: &str, flavor: PlainFlavor) -> BTreeMap<String, String> {
    match flavor {
        PlainFlavor::Alacritty => parse_alacritty(contents),
        PlainFlavor::WezTerm => parse_wezterm(contents),
        PlainFlavor::Ghostty | PlainFlavor::Kitty => parse_key_value(contents, flavor),
    }
}

fn parse_key_value(contents: &str, flavor: PlainFlavor) -> BTreeMap<String, String> {
    let mut colors = BTreeMap::new();
    for line in contents.lines() {
        let line = strip_config_comment(line);
        if line.is_empty() {
            continue;
        }
        let (key, value) = if let Some((key, value)) = line.split_once('=') {
            (key.trim(), value.trim())
        } else {
            let mut fields = line.split_whitespace();
            (fields.next().unwrap_or(""), fields.next().unwrap_or(""))
        };
        if matches!(flavor, PlainFlavor::Ghostty) && key == "palette" {
            if let Some((index, value)) = value.split_once('=')
                && let (Ok(index), Some(color)) = (index.trim().parse::<usize>(), parse_hex(value))
                && let Some(name) = COLOR_NAMES.get(index)
            {
                colors.insert((*name).to_owned(), color);
            }
            continue;
        }
        let target = match key {
            "background" => Some("background"),
            "foreground" => Some("foreground"),
            "cursor" | "cursor-color" => Some("cursor"),
            "selection_background" | "selection-background" => Some("selection"),
            _ => key.strip_prefix("color").and_then(|index| {
                index
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| COLOR_NAMES.get(index).copied())
            }),
        };
        if let (Some(target), Some(color)) = (target, parse_hex(value)) {
            colors.insert(target.to_owned(), color);
        }
    }
    colors
}

fn parse_alacritty(contents: &str) -> BTreeMap<String, String> {
    let mut colors = BTreeMap::new();
    let mut section = String::new();
    for line in contents.lines() {
        let line = strip_config_comment(line);
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(['[', ']']).to_owned();
            continue;
        }
        if line.ends_with(':') && !line.contains(['{', '}']) {
            let part = line.trim_end_matches(':').trim();
            section = if part == "colors" {
                "colors".to_owned()
            } else {
                format!("colors.{part}")
            };
            continue;
        }
        let Some((key, value)) = line.split_once(['=', ':']) else {
            continue;
        };
        let key = key.trim();
        let target = match (section.as_str(), key) {
            ("colors.primary", "background") => Some("background"),
            ("colors.primary", "foreground") => Some("foreground"),
            ("colors.cursor", "cursor") => Some("cursor"),
            ("colors.selection", "background") => Some("selection"),
            ("colors.normal", name) => ansi_name(name, false),
            ("colors.bright", name) => ansi_name(name, true),
            _ => None,
        };
        if let (Some(target), Some(color)) = (target, parse_hex(value)) {
            colors.insert(target.to_owned(), color);
        }
    }
    colors
}

fn parse_wezterm(contents: &str) -> BTreeMap<String, String> {
    let mut colors = BTreeMap::new();
    for (key, target) in [
        ("background", "background"),
        ("foreground", "foreground"),
        ("cursor_bg", "cursor"),
        ("selection_bg", "selection"),
    ] {
        if let Some(value) = lua_assignment(contents, key).and_then(parse_hex) {
            colors.insert(target.to_owned(), value);
        }
    }
    for (key, offset) in [("ansi", 0), ("brights", 8)] {
        if let Some(values) = lua_array(contents, key) {
            for (index, value) in values.into_iter().take(8).enumerate() {
                if let Some(color) = parse_hex(value) {
                    colors.insert(COLOR_NAMES[index + offset].to_owned(), color);
                }
            }
        }
    }
    colors
}

fn lua_assignment<'a>(contents: &'a str, key: &str) -> Option<&'a str> {
    contents.lines().find_map(|line| {
        let (candidate, value) = line.split_once('=')?;
        (candidate.trim() == key).then_some(value.trim().trim_end_matches(','))
    })
}

fn lua_array<'a>(contents: &'a str, key: &str) -> Option<Vec<&'a str>> {
    let start = contents.find(key)?;
    let body = contents[start..].split_once('{')?.1.split_once('}')?.0;
    Some(
        body.split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .collect(),
    )
}

fn ansi_name(name: &str, bright: bool) -> Option<&'static str> {
    let index = [
        "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    ]
    .iter()
    .position(|candidate| *candidate == name)?;
    COLOR_NAMES.get(index + usize::from(bright) * 8).copied()
}

fn candidate(
    source: &'static str,
    profile: String,
    colors: BTreeMap<String, String>,
) -> Option<Candidate> {
    (colors.contains_key("background") && colors.contains_key("foreground")).then_some(Candidate {
        source,
        profile,
        colors,
        terminal_style: None,
    })
}

fn palette_from_colors(colors: &BTreeMap<String, String>) -> TerminalPalette {
    let fallback = graphite_colors();
    let color = |name: &str| {
        colors
            .get(name)
            .cloned()
            .unwrap_or_else(|| fallback[name].clone())
    };
    let cursor = colors
        .get("cursor")
        .or_else(|| colors.get("foreground"))
        .cloned()
        .unwrap_or_else(|| fallback["cursor"].clone());
    TerminalPalette {
        background: color("background"),
        foreground: color("foreground"),
        cursor,
        selection: color("selection"),
        black: color("black"),
        red: color("red"),
        green: color("green"),
        yellow: color("yellow"),
        blue: color("blue"),
        magenta: color("magenta"),
        cyan: color("cyan"),
        white: color("white"),
        bright_black: color("brightBlack"),
        bright_red: color("brightRed"),
        bright_green: color("brightGreen"),
        bright_yellow: color("brightYellow"),
        bright_blue: color("brightBlue"),
        bright_magenta: color("brightMagenta"),
        bright_cyan: color("brightCyan"),
        bright_white: color("brightWhite"),
    }
}

fn graphite_colors() -> BTreeMap<String, String> {
    [
        ("background", "#202326"),
        ("foreground", "#D7D9D5"),
        ("cursor", "#A7C7B7"),
        ("selection", "#3A4B52"),
        ("black", "#353A3E"),
        ("red", "#D8877E"),
        ("green", "#8FBF8F"),
        ("yellow", "#D6B56E"),
        ("blue", "#82AFC5"),
        ("magenta", "#B69AC8"),
        ("cyan", "#7EB7B3"),
        ("white", "#C9CCC7"),
        ("brightBlack", "#687078"),
        ("brightRed", "#E79A90"),
        ("brightGreen", "#A4D4A4"),
        ("brightYellow", "#E6C985"),
        ("brightBlue", "#9CC5D8"),
        ("brightMagenta", "#CCB2DA"),
        ("brightCyan", "#98CFCC"),
        ("brightWhite", "#F2F2EE"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

fn parse_hex(value: &str) -> Option<String> {
    let value = value
        .split_whitespace()
        .next()?
        .trim_matches(['\'', '"', ',']);
    let value = value.strip_prefix("0x").unwrap_or(value);
    let value = value.strip_prefix('#').unwrap_or(value);
    match value.len() {
        6 if value.chars().all(|character| character.is_ascii_hexdigit()) => {
            Some(format!("#{}", value.to_ascii_uppercase()))
        }
        3 if value.chars().all(|character| character.is_ascii_hexdigit()) => {
            let expanded = value
                .chars()
                .flat_map(|character| [character, character])
                .collect::<String>();
            Some(format!("#{}", expanded.to_ascii_uppercase()))
        }
        _ => None,
    }
}

fn strip_config_comment(line: &str) -> &str {
    let line = line.trim();
    if line.starts_with('#') {
        return "";
    }
    line
}

fn rgb_hex(red: f64, green: f64, blue: f64) -> String {
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        channel(red),
        channel(green),
        channel(blue)
    )
}

fn plutil_raw_file(path: &Path, key: &str) -> Option<String> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(path)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn plutil_number_file(path: &Path, key: &str) -> Option<f64> {
    plutil_raw_file(path, key)?.parse().ok()
}

fn plutil_bool_file(path: &Path, key: &str) -> Option<bool> {
    match plutil_raw_file(path, key)?.as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

fn plutil_raw_stdin(input: &[u8], key: &str) -> Option<String> {
    let mut child = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(input).ok()?;
    let output = child.wait_with_output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    use std::fs;
    #[cfg(target_os = "macos")]
    use tempfile::TempDir;

    #[test]
    #[cfg(target_os = "macos")]
    fn imports_real_iterm2_plist_shape() {
        let home = TempDir::new().expect("temp home");
        let preferences = home.path().join("Library/Preferences");
        fs::create_dir_all(&preferences).expect("preferences");
        fs::write(
            preferences.join("com.googlecode.iterm2.plist"),
            iterm_fixture(),
        )
        .expect("fixture");

        let report = import_terminal_theme_from(home.path());

        assert!(report.imported);
        assert_eq!(report.source.as_deref(), Some("iTerm2"));
        assert_eq!(report.profile.as_deref(), Some("Fixture Solar"));
        let palette = report.palette.expect("palette");
        assert_eq!(palette.background, "#1A334D");
        assert_eq!(palette.foreground, "#E6CCB3");
        assert_eq!(palette.cursor, "#33CC99");
        assert_eq!(palette.bright_white, "#F5F5F5");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn imports_terminal_app_keyed_archive_fixture() {
        let home = TempDir::new().expect("temp home");
        let preferences = home.path().join("Library/Preferences");
        fs::create_dir_all(&preferences).expect("preferences");
        let color_archive = terminal_color_archive("0.12 0.24 0.36 1\0");
        let foreground_archive = terminal_color_archive("0.9 0.8 0.7 1\0");
        let cursor_archive = terminal_color_archive("0.2 0.8 0.6 1\0");
        let red_archive = terminal_color_archive("0.75 0.25 0.2 1\0");
        fs::write(
            preferences.join("com.apple.Terminal.plist"),
            terminal_fixture(
                &color_archive,
                &foreground_archive,
                &cursor_archive,
                &red_archive,
            ),
        )
        .expect("fixture");

        let report = import_terminal_theme_from(home.path());

        assert!(report.imported);
        assert_eq!(report.source.as_deref(), Some("Terminal.app"));
        assert_eq!(report.profile.as_deref(), Some("Fixture Dark"));
        let encoded = serde_json::to_value(&report).expect("serialized import");
        assert!(encoded.get("terminalStyle").is_some());
        assert!(encoded.get("terminal_style").is_none());
        let palette = report.palette.expect("palette");
        assert_eq!(palette.background, "#1F3D5C");
        assert_eq!(palette.foreground, "#E6CCB3");
        assert_eq!(palette.cursor, "#33CC99");
        assert_eq!(palette.red, "#BF4033");
        let style = report.terminal_style.expect("terminal style");
        assert_eq!(style.font_family.as_deref(), Some("FixtureMono-Regular"));
        assert_eq!(style.font_size, Some(12.0));
        assert_eq!(style.draw_bold_text_in_bright_colors, Some(false));
    }

    #[test]
    fn parses_plain_terminal_formats() {
        let ghostty = "background = #112233\nforeground = #ddeeff\npalette = 1=#cc5544";
        let kitty = "background #101820\nforeground #f0eee8\ncolor2 #68a06d";
        let alacritty = "[colors.primary]\nbackground = '#172026'\nforeground = '#d6d8d5'\n[colors.normal]\nred = '#b95c58'";
        let wezterm = "background = '#182128',\nforeground = '#e2e0da',\nansi = { '#111111', '#aa4444' },\nbrights = { '#777777', '#ee7777' },";
        assert_eq!(parse_plain(ghostty, PlainFlavor::Ghostty)["red"], "#CC5544");
        assert_eq!(parse_plain(kitty, PlainFlavor::Kitty)["green"], "#68A06D");
        assert_eq!(
            parse_plain(alacritty, PlainFlavor::Alacritty)["background"],
            "#172026"
        );
        assert_eq!(
            parse_plain(alacritty, PlainFlavor::Alacritty)["red"],
            "#B95C58"
        );
        assert_eq!(
            parse_plain(wezterm, PlainFlavor::WezTerm)["brightRed"],
            "#EE7777"
        );
    }

    #[cfg(target_os = "macos")]
    fn iterm_fixture() -> String {
        let color = |red: f64, green: f64, blue: f64| {
            format!(
                "<dict><key>Red Component</key><real>{red}</real><key>Green Component</key><real>{green}</real><key>Blue Component</key><real>{blue}</real></dict>"
            )
        };
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Default Bookmark Guid</key><string>fixture-guid</string>
<key>New Bookmarks</key><array><dict>
<key>Guid</key><string>fixture-guid</string><key>Name</key><string>Fixture Solar</string>
<key>Background Color</key>{}<key>Foreground Color</key>{}<key>Cursor Color</key>{}<key>Selection Color</key>{}
{}
</dict></array></dict></plist>"#,
            color(0.1, 0.2, 0.3),
            color(0.9, 0.8, 0.7),
            color(0.2, 0.8, 0.6),
            color(0.25, 0.3, 0.35),
            (0..16)
                .map(|index| {
                    let component = 0.06 * (index as f64 + 1.0);
                    format!(
                        "<key>Ansi {index} Color</key>{}",
                        color(component, component, component)
                    )
                })
                .collect::<String>()
        )
    }

    #[cfg(target_os = "macos")]
    fn terminal_color_archive(rgb: &str) -> String {
        let data = BASE64.encode(rgb.as_bytes());
        let archive = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>$objects</key><array><string>$null</string><dict><key>NSRGB</key><data>{data}</data></dict></array></dict></plist>"#
        );
        BASE64.encode(archive.as_bytes())
    }

    #[cfg(target_os = "macos")]
    fn terminal_fixture(background: &str, foreground: &str, cursor: &str, red: &str) -> String {
        let font = terminal_font_archive("FixtureMono-Regular", 12.0);
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Default Window Settings</key><string>Fixture Dark</string>
<key>Window Settings</key><dict><key>Fixture Dark</key><dict>
<key>BackgroundColor</key><data>{background}</data>
<key>TextColor</key><data>{foreground}</data>
<key>CursorColor</key><data>{cursor}</data>
<key>ANSIRedColor</key><data>{red}</data>
<key>Font</key><data>{font}</data>
</dict></dict></dict></plist>"#
        )
    }

    #[cfg(target_os = "macos")]
    fn terminal_font_archive(name: &str, size: f64) -> String {
        let archive = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>$objects</key><array>
<string>$null</string><dict><key>NSSize</key><real>{size}</real></dict><string>{name}</string>
</array></dict></plist>"#
        );
        BASE64.encode(archive.as_bytes())
    }
}
