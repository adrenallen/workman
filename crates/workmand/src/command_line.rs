//! Splits configured command strings the way the launching shell reads them.
//!
//! Configured commands execute through the login shell's `-c` — POSIX shells on
//! Unix, PowerShell on Windows — and Workman also inspects them for health
//! checks and safe rewrites. Quote handling matches both worlds. The backslash
//! is an escape character only on Unix: on Windows it is the path separator,
//! and PowerShell reads it literally.

/// Split strictly: an unterminated quote or trailing escape is an error, so
/// command rewrites never operate on a half-parsed command.
pub(crate) fn split(source: &str) -> Result<Vec<String>, String> {
    let (words, unterminated) = tokenize(source);
    if unterminated {
        return Err("command has an unterminated quote or escape".to_owned());
    }
    Ok(words)
}

/// Split permissively for diagnostics: a malformed tail still yields the words
/// accumulated so far, matching how the runtime doctor reads commands.
pub(crate) fn split_permissive(source: &str) -> Vec<String> {
    tokenize(source).0
}

fn tokenize(source: &str) -> (Vec<String>, bool) {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;
    for character in source.chars() {
        if escaped {
            word.push(character);
            escaped = false;
            started = true;
        } else if character == '\\' && quote != Some('\'') && cfg!(unix) {
            escaped = true;
            started = true;
        } else if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            } else {
                word.push(character);
            }
            started = true;
        } else if character.is_whitespace() && quote.is_none() {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
            }
        } else {
            word.push(character);
            started = true;
        }
    }
    let unterminated = escaped || quote.is_some();
    if escaped {
        word.push('\\');
    }
    if started {
        words.push(word);
    }
    (words, unterminated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_group_words_and_adjacent_segments_concatenate() {
        assert_eq!(
            split("claude --flag 'two words' a\"b\"c").unwrap(),
            ["claude", "--flag", "two words", "abc"]
        );
    }

    #[test]
    fn unterminated_quotes_error_strictly_but_stay_readable_for_diagnostics() {
        assert!(split("kimi 'oops").is_err());
        assert_eq!(split_permissive("kimi 'oops"), ["kimi", "oops"]);
    }

    #[cfg(windows)]
    #[test]
    fn windows_paths_keep_their_backslashes() {
        assert_eq!(
            split(r"C:\Users\dev\.kimi-code\bin\kimi.exe --yolo").unwrap(),
            [r"C:\Users\dev\.kimi-code\bin\kimi.exe", "--yolo"]
        );
        assert_eq!(
            split(r"'C:\Program Files\Kimi\kimi.exe' --yolo").unwrap(),
            [r"C:\Program Files\Kimi\kimi.exe", "--yolo"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_backslashes_escape_the_next_character() {
        assert_eq!(
            split(r"claude\ code --flag").unwrap(),
            ["claude code", "--flag"]
        );
        assert!(split(r"claude \").is_err());
    }
}
