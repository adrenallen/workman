use tauri::{Runtime, Url, plugin::TauriPlugin};

#[derive(Debug, Eq, PartialEq)]
enum NavigationAction {
    Allow,
    OpenExternally,
    Reject,
}

fn navigation_action(url: &Url) -> NavigationAction {
    match url.scheme() {
        "tauri" | "about" => NavigationAction::Allow,
        "http" | "https" if is_dev_app_url(url) => NavigationAction::Allow,
        "http" | "https" => NavigationAction::OpenExternally,
        _ => NavigationAction::Reject,
    }
}

fn is_dev_app_url(url: &Url) -> bool {
    url.host_str() == Some("tauri.localhost")
        || (cfg!(debug_assertions)
            && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
            && url.port() == Some(1420))
}

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri::plugin::Builder::new("external-navigation")
        .on_navigation(|_webview, url| match navigation_action(url) {
            NavigationAction::Allow => true,
            NavigationAction::OpenExternally => {
                if let Err(error) = super::open_in_browser(url.as_str()) {
                    eprintln!("could not open external navigation in browser: {error}");
                }
                false
            }
            NavigationAction::Reject => {
                eprintln!("refused embedded webview navigation to unsupported URL: {url}");
                false
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_allows_only_app_pages_and_redirects_web_links() {
        assert_eq!(
            navigation_action(&Url::parse("tauri://localhost/").unwrap()),
            NavigationAction::Allow
        );
        assert_eq!(
            navigation_action(&Url::parse("http://127.0.0.1:1420/todos").unwrap()),
            NavigationAction::Allow
        );
        assert_eq!(
            navigation_action(&Url::parse("http://tauri.localhost/").unwrap()),
            NavigationAction::Allow
        );
        assert_eq!(
            navigation_action(&Url::parse("https://example.com/docs").unwrap()),
            NavigationAction::OpenExternally
        );
        assert_eq!(
            navigation_action(&Url::parse("http://localhost:4000/not-the-app").unwrap()),
            NavigationAction::OpenExternally
        );
        assert_eq!(
            navigation_action(&Url::parse("file:///tmp/private").unwrap()),
            NavigationAction::Reject
        );
        assert_eq!(
            navigation_action(&Url::parse("javascript:alert(1)").unwrap()),
            NavigationAction::Reject
        );
        assert_eq!(
            navigation_action(&Url::parse("mailto:test@example.com").unwrap()),
            NavigationAction::Reject
        );
    }
}
