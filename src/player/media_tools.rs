use crate::utils::error::BotError;

const LOG_EXCERPT_LIMIT: usize = 240;

pub fn build_ytdlp_user_args() -> Vec<String> {
    build_ytdlp_user_args_from_env(std::env::var("POT_SERVER_URL").ok().as_deref())
}

fn build_ytdlp_user_args_from_env(pot_server_url: Option<&str>) -> Vec<String> {
    let Some(pot_server_url) = pot_server_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };

    vec![
        "--extractor-args".to_string(),
        format!("youtubepot-bgutilhttp:base_url={pot_server_url}"),
    ]
}

pub fn classify_media_error(text: &str) -> BotError {
    let normalized = text.to_lowercase();

    if normalized.contains("sign in to confirm you're not a bot") {
        BotError::AntiBotChallenge
    } else if normalized.contains("http error 429")
        || normalized.contains("too many requests")
        || normalized.contains("rate limit")
    {
        BotError::RateLimited
    } else if normalized.contains("http error 403")
        || normalized.contains("403 forbidden")
        || normalized.contains("forbidden")
    {
        BotError::MediaForbidden
    } else if normalized.contains("connection reset by peer")
        || normalized.contains("network is unreachable")
        || normalized.contains("tls")
        || normalized.contains("i/o error")
        || normalized.contains("connection timed out")
    {
        BotError::StreamNetwork(log_excerpt(text))
    } else {
        BotError::ExtractorFailed(log_excerpt(text))
    }
}

pub fn log_excerpt(text: &str) -> String {
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut excerpt = flattened.trim().to_string();

    if excerpt.chars().count() <= LOG_EXCERPT_LIMIT {
        return excerpt;
    }

    let truncated: String = excerpt.chars().take(LOG_EXCERPT_LIMIT).collect();
    excerpt = truncated.trim_end().to_string();
    excerpt.push_str("...");
    excerpt
}

#[cfg(test)]
mod tests {
    use super::{build_ytdlp_user_args_from_env, classify_media_error};
    use crate::utils::error::BotError;

    #[test]
    fn ytdlp_user_args_are_empty_without_pot_server() {
        assert!(build_ytdlp_user_args_from_env(None).is_empty());
        assert!(build_ytdlp_user_args_from_env(Some("   ")).is_empty());
    }

    #[test]
    fn ytdlp_user_args_include_pot_server() {
        assert_eq!(
            build_ytdlp_user_args_from_env(Some("http://pot-server:4416")),
            vec![
                "--extractor-args".to_string(),
                "youtubepot-bgutilhttp:base_url=http://pot-server:4416".to_string(),
            ]
        );
    }

    #[test]
    fn media_error_classification_maps_common_failures() {
        assert!(matches!(
            classify_media_error("Sign in to confirm you're not a bot"),
            BotError::AntiBotChallenge
        ));
        assert!(matches!(
            classify_media_error("ERROR: HTTP Error 403: Forbidden"),
            BotError::MediaForbidden
        ));
        assert!(matches!(
            classify_media_error("HTTP Error 429: Too Many Requests"),
            BotError::RateLimited
        ));
        assert!(matches!(
            classify_media_error("Connection reset by peer while reading"),
            BotError::StreamNetwork(_)
        ));
    }
}
