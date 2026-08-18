/// Return the user-visible portion of a project title.
///
/// Creation treats an empty value as no explicit title, while rename callers
/// turn `None` into their existing `invalid_project_name` response.
pub(crate) fn normalized_project_title(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

pub(crate) fn normalized_optional_project_title(value: Option<&str>) -> Option<String> {
    value.and_then(normalized_project_title).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_titles_trim_and_treat_whitespace_as_absent() {
        assert_eq!(
            normalized_project_title("  Client workspace  "),
            Some("Client workspace")
        );
        assert_eq!(normalized_project_title("\n\t  "), None);
        assert_eq!(
            normalized_optional_project_title(Some("  Checkout title  ")).as_deref(),
            Some("Checkout title")
        );
        assert_eq!(normalized_optional_project_title(Some("   ")), None);
        assert_eq!(normalized_optional_project_title(None), None);
    }
}
