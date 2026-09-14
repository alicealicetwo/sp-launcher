//! The Play tab's news feed, fetched from `Config::news_url`.
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

/// Fetches and parses the feed. An empty `news_url` is not an error — it just
/// means no news is configured yet — and returns an empty feed.
pub async fn fetch(news_url: &str) -> Result<Vec<NewsItem>> {
    if news_url.is_empty() {
        return Ok(Vec::new());
    }
    let items = reqwest::get(news_url)
        .await?
        .error_for_status()?
        .json::<Vec<NewsItem>>()
        .await?;
    Ok(items)
}
