//! The Play tab's news feed.
//!
//! The feed is just a JSON array of `NewsItem`; this module only defines its
//! shape and how to fetch it. `starts_at`/`ends_at` are informational —
//! whether an item is currently active is decided by the frontend, so a
//! launcher upgrade is never required to change what's showing.

use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsItem {
    pub id: String,
    pub tag: String,
    pub title: String,
    pub description: String,
    /// URL of the slide's background image.
    pub image: String,
    /// ISO-8601. Empty means "already started".
    #[serde(default)]
    pub starts_at: String,
    /// ISO-8601. Empty means "never ends".
    #[serde(default)]
    pub ends_at: String,
    pub clickable: bool,
    /// Only meaningful when `clickable` is true.
    #[serde(default)]
    pub url: Option<String>,
}

/// Where the feed lives. Fixed rather than configurable: it is this
/// server's feed, and a player pointing the launcher at some other URL only
/// ever breaks their own news panel.
pub const FEED_URL: &str = "http://64.226.112.204/launcher/news.json";

/// Fetches and parses the feed from `url` (in practice always `FEED_URL`;
/// taking it as an argument keeps this testable against a local server). An
/// empty url yields an empty feed rather than an error.
pub async fn fetch(url: &str) -> Result<Vec<NewsItem>> {
    if url.is_empty() {
        return Ok(Vec::new());
    }
    let items = reqwest::get(url)
        .await?
        .error_for_status()?
        .json::<Vec<NewsItem>>()
        .await?;
    Ok(items)
}
