use songbird::input::{ChildContainer, Input};
use std::io::{BufRead, BufReader};
use std::process::{Child, ChildStderr, Stdio};
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::utils::error::{BotError, BotResult};

const LOG_EXCERPT_LIMIT: usize = 240;
const YTDLP_AUDIO_FORMAT: &str = "ba[abr>0][vcodec=none]/best";

#[derive(Clone, Copy)]
pub enum MediaTool {
    Ffmpeg,
    YtDlp,
}

pub fn build_ytdlp_user_args() -> Vec<String> {
    build_ytdlp_user_args_from_env(
        std::env::var("POT_SERVER_URL").ok().as_deref(),
        std::env::var("YTDLP_WPC_BROWSER_PATH").ok().as_deref(),
    )
}

fn build_ytdlp_user_args_from_env(
    pot_server_url: Option<&str>,
    wpc_browser_path: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "--extractor-args".to_string(),
        "youtube:player_client=mweb,web_safari,web".to_string(),
    ];

    let wpc_browser_path = wpc_browser_path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("/usr/bin/chromium");

    args.extend([
        "--extractor-args".to_string(),
        format!("youtubepot-wpc:browser_path={wpc_browser_path}"),
    ]);

    if let Some(pot_server_url) = pot_server_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        args.extend([
            "--extractor-args".to_string(),
            format!("youtubepot-bgutilhttp:base_url={pot_server_url}"),
        ]);
    }

    args
}

pub fn build_ytdlp_playback_args(url: &str) -> Vec<String> {
    let mut args = build_ytdlp_user_args();
    args.extend([
        "-f".to_string(),
        YTDLP_AUDIO_FORMAT.to_string(),
        "--no-playlist".to_string(),
        "--no-progress".to_string(),
        "-o".to_string(),
        "-".to_string(),
        url.to_string(),
    ]);
    args
}

pub fn create_ytdlp_direct_source(url: &str, attempt_id: Option<u64>) -> BotResult<Input> {
    let child = spawn_ytdlp_media_child(url, attempt_id, "direct-playback")?;
    Ok(ChildContainer::from(child).into())
}

pub fn spawn_ytdlp_media_child(
    url: &str,
    attempt_id: Option<u64>,
    purpose: &'static str,
) -> BotResult<Child> {
    let started_at = Instant::now();
    info!(
        "Spawning yt-dlp media download: attempt_id={attempt_id:?}, purpose={purpose}, url={url}"
    );

    let mut child = std::process::Command::new("yt-dlp")
        .args(build_ytdlp_playback_args(url))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            warn!(
                "Failed to spawn yt-dlp media download after {:?}: attempt_id={attempt_id:?}, purpose={purpose}, error={e}",
                started_at.elapsed()
            );
            BotError::ExtractorFailed(log_excerpt(&format!("Failed to spawn yt-dlp: {e}")))
        })?;

    drain_child_stderr(child.stderr.take(), MediaTool::YtDlp, attempt_id, purpose);

    info!(
        "yt-dlp media download spawned in {:?}: attempt_id={attempt_id:?}, purpose={purpose}",
        started_at.elapsed()
    );
    Ok(child)
}

pub fn drain_child_stderr(
    stderr: Option<ChildStderr>,
    tool: MediaTool,
    attempt_id: Option<u64>,
    purpose: &'static str,
) {
    let Some(stderr) = stderr else {
        return;
    };

    tokio::task::spawn_blocking(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            log_media_tool_line(tool, attempt_id, purpose, &line);
        }
    });
}

fn log_media_tool_line(tool: MediaTool, attempt_id: Option<u64>, purpose: &str, line: &str) {
    match classify_media_error(line) {
        BotError::AntiBotChallenge
        | BotError::RateLimited
        | BotError::MediaForbidden
        | BotError::StreamNetwork(_) => warn_media_tool_line(tool, attempt_id, purpose, line),
        _ if is_media_tool_info_line(line) => info_media_tool_line(tool, attempt_id, purpose, line),
        _ if is_media_tool_diagnostic_line(line) => {
            warn_media_tool_line(tool, attempt_id, purpose, line)
        }
        _ => debug_media_tool_line(tool, attempt_id, purpose, line),
    }
}

fn info_media_tool_line(tool: MediaTool, attempt_id: Option<u64>, purpose: &str, line: &str) {
    match tool {
        MediaTool::Ffmpeg => {
            info!(target: "ffmpeg", "attempt_id={attempt_id:?} purpose={purpose} {line}");
        }
        MediaTool::YtDlp => {
            info!(target: "yt-dlp", "attempt_id={attempt_id:?} purpose={purpose} {line}");
        }
    }
}

fn warn_media_tool_line(tool: MediaTool, attempt_id: Option<u64>, purpose: &str, line: &str) {
    match tool {
        MediaTool::Ffmpeg => {
            warn!(target: "ffmpeg", "attempt_id={attempt_id:?} purpose={purpose} {line}");
        }
        MediaTool::YtDlp => {
            warn!(target: "yt-dlp", "attempt_id={attempt_id:?} purpose={purpose} {line}");
        }
    }
}

fn debug_media_tool_line(tool: MediaTool, attempt_id: Option<u64>, purpose: &str, line: &str) {
    match tool {
        MediaTool::Ffmpeg => {
            debug!(target: "ffmpeg", "attempt_id={attempt_id:?} purpose={purpose} {line}");
        }
        MediaTool::YtDlp => {
            debug!(target: "yt-dlp", "attempt_id={attempt_id:?} purpose={purpose} {line}");
        }
    }
}

fn is_media_tool_info_line(line: &str) -> bool {
    line.contains("Sleeping") || line.contains("Generating a gvs PO Token")
}

pub fn is_media_tool_diagnostic_line(line: &str) -> bool {
    let line = line.to_lowercase();
    [
        "buffer",
        "delay",
        "drop",
        "error",
        "failed",
        "forbidden",
        "http",
        "i/o",
        "non-monotonous",
        "reconnect",
        "reset",
        "timeout",
        "underrun",
        "warning",
    ]
    .iter()
    .any(|needle| line.contains(needle))
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
    use super::{build_ytdlp_playback_args, build_ytdlp_user_args_from_env, classify_media_error};
    use crate::utils::error::BotError;

    #[test]
    fn ytdlp_user_args_include_wpc_without_pot_server() {
        assert_eq!(
            build_ytdlp_user_args_from_env(None, None),
            vec![
                "--extractor-args".to_string(),
                "youtube:player_client=mweb,web_safari,web".to_string(),
                "--extractor-args".to_string(),
                "youtubepot-wpc:browser_path=/usr/bin/chromium".to_string(),
            ]
        );
    }

    #[test]
    fn ytdlp_user_args_include_wpc_and_pot_server() {
        assert_eq!(
            build_ytdlp_user_args_from_env(
                Some("http://pot-server:4416"),
                Some("/custom/chromium")
            ),
            vec![
                "--extractor-args".to_string(),
                "youtube:player_client=mweb,web_safari,web".to_string(),
                "--extractor-args".to_string(),
                "youtubepot-wpc:browser_path=/custom/chromium".to_string(),
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

    #[test]
    fn ytdlp_playback_args_stream_to_stdout_without_json_url_handoff() {
        let args = build_ytdlp_playback_args("https://example.com/watch?v=abc");

        assert!(args.contains(&"-o".to_string()));
        assert!(args.windows(2).any(|window| window == ["-o", "-"]));
        assert!(
            args.windows(2)
                .any(|window| { window == ["-f", "ba[abr>0][vcodec=none]/best"] })
        );
        assert!(args.contains(&"--no-playlist".to_string()));
        assert!(!args.contains(&"-j".to_string()));
    }
}
