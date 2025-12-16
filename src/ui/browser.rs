//! Browser opening abstraction for testability.
//!
//! Provides a trait-based approach to opening URLs in the browser,
//! allowing tests to use mock implementations instead of actually
//! launching browsers.

use std::process::Command;

/// Trait for opening URLs in the browser.
pub trait BrowserOpener: Send + Sync {
    /// Opens a URL in the system's default browser.
    fn open_url(&self, url: &str);
}

/// Production implementation that actually opens the browser.
pub struct SystemBrowserOpener;

impl BrowserOpener for SystemBrowserOpener {
    fn open_url(&self, url: &str) {
        #[cfg(target_os = "macos")]
        let _ = Command::new("open").arg(url).spawn();

        #[cfg(target_os = "linux")]
        let _ = Command::new("xdg-open").arg(url).spawn();

        #[cfg(target_os = "windows")]
        let _ = Command::new("cmd").args(&["/C", "start", url]).spawn();
    }
}

/// Mock implementation for tests that tracks opened URLs without launching browsers.
#[cfg(test)]
pub struct MockBrowserOpener {
    pub opened_urls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

#[cfg(test)]
impl Default for MockBrowserOpener {
    fn default() -> Self {
        Self {
            opened_urls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

#[cfg(test)]
impl MockBrowserOpener {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_opened_urls(&self) -> Vec<String> {
        self.opened_urls.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl BrowserOpener for MockBrowserOpener {
    fn open_url(&self, url: &str) {
        self.opened_urls.lock().unwrap().push(url.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_browser_opener() {
        let opener = MockBrowserOpener::new();

        opener.open_url("https://example.com/pr/123");
        opener.open_url("https://example.com/wi/456");

        let urls = opener.get_opened_urls();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://example.com/pr/123");
        assert_eq!(urls[1], "https://example.com/wi/456");
    }
}
