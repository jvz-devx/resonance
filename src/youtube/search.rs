use std::time::Duration;

use rusty_ytdl::search::{SearchResult, YouTube};
use tracing::debug;

use crate::utils::error::{BotError, BotResult};

/// Search YouTube and return up to `limit` results as (title, url, duration) tuples.
pub async fn search_youtube(
    query: &str,
    limit: usize,
) -> BotResult<Vec<(String, String, Option<Duration>)>> {
    let youtube = YouTube::new().map_err(|e| BotError::SearchFailed(e.to_string()))?;

    let results = youtube
        .search(query, None)
        .await
        .map_err(|e| BotError::SearchFailed(e.to_string()))?;

    let mut output = Vec::new();

    for result in results.into_iter().take(limit) {
        match result {
            SearchResult::Video(video) => {
                let duration = if video.duration > 0 {
                    Some(Duration::from_millis(video.duration))
                } else {
                    None
                };

                let url = format!("https://www.youtube.com/watch?v={}", video.id);
                debug!("Search result: {} ({})", video.title, url);
                output.push((video.title, url, duration));
            }
            _ => continue, // Skip playlists and channels
        }
    }

    if output.is_empty() {
        return Err(BotError::NoResults);
    }

    Ok(output)
}

/// Resolve a query to a single YouTube URL.
/// If the query is already a URL, returns it as-is.
/// Otherwise, searches and returns the first result URL.
pub async fn resolve_query(query: &str) -> BotResult<(String, String, Option<Duration>)> {
    if is_youtube_url(query) {
        // Fetch video info for the URL
        let video = rusty_ytdl::Video::new(query)
            .map_err(|e| BotError::SearchFailed(e.to_string()))?;

        let info = video
            .get_basic_info()
            .await
            .map_err(|e| BotError::SearchFailed(e.to_string()))?;

        let title = info.video_details.title.clone();
        let duration = info
            .video_details
            .length_seconds
            .parse::<u64>()
            .ok()
            .map(Duration::from_secs);

        Ok((title, query.to_string(), duration))
    } else {
        // Search and take first result
        let results = search_youtube(query, 1).await?;
        results
            .into_iter()
            .next()
            .ok_or(BotError::NoResults)
    }
}

/// Check if a string looks like a YouTube URL
pub fn is_youtube_url(s: &str) -> bool {
    s.contains("youtube.com/watch")
        || s.contains("youtu.be/")
        || s.contains("youtube.com/playlist")
        || s.contains("music.youtube.com/watch")
        || s.contains("youtube.com/shorts/")
        || s.contains("youtube.com/live/")
}
