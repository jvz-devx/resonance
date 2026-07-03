use songbird::input::{ChildContainer, Input};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tracing::{info, warn};

use crate::player::media_tools::{
    MediaTool, drain_child_stderr, log_excerpt, spawn_ytdlp_media_child,
};
use crate::utils::error::{BotError, BotResult};

/// Create an audio source that pipes through ffmpeg with dynamic audio normalization.
///
/// Spawns yt-dlp as the downloader, then pipes it through ffmpeg to normalize
/// the audio, outputting Opus-in-Ogg on stdout. yt-dlp owns the media request so
/// YouTube-required waits, PO tokens, and downloader quirks are honored.
pub async fn create_normalized_source_with_timeout(
    url: &str,
    _ytdl_timeout: Duration,
    attempt_id: Option<u64>,
) -> BotResult<Input> {
    let mut ytdlp_child = spawn_ytdlp_media_child(url, attempt_id, "normalized-playback")?;
    let ytdlp_stdout = ytdlp_child
        .stdout
        .take()
        .ok_or_else(|| BotError::ExtractorFailed("yt-dlp stdout was not piped".to_string()))?;

    let mut ffmpeg_args: Vec<String> = vec![
        "-nostdin".to_string(),
        "-i".to_string(),
        "pipe:0".to_string(),
    ];
    ffmpeg_args.extend([
        "-af".to_string(),
        // dynaudnorm has near-zero startup latency, unlike loudnorm (~3s)
        "dynaudnorm=f=250:g=15".to_string(),
        "-ar".to_string(),
        "48000".to_string(),
        "-ac".to_string(),
        "2".to_string(),
        "-c:a".to_string(),
        "libopus".to_string(),
        "-b:a".to_string(),
        "96k".to_string(),
        "-f".to_string(),
        "ogg".to_string(),
        "pipe:1".to_string(),
    ]);

    let ffmpeg_started_at = Instant::now();
    info!(
        "Spawning ffmpeg with dynaudnorm + opus/ogg for normalized playback: attempt_id={:?}",
        attempt_id
    );
    let mut child = std::process::Command::new("ffmpeg")
        .args(&ffmpeg_args)
        .stdin(Stdio::from(ytdlp_stdout))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            let _ = ytdlp_child.kill();
            let _ = ytdlp_child.wait();
            warn!(
                "Failed to spawn ffmpeg for normalized playback after {:?}: attempt_id={:?}, error={e}",
                ffmpeg_started_at.elapsed(),
                attempt_id
            );
            BotError::FfmpegSetupFailed(log_excerpt(&format!("Failed to spawn ffmpeg: {e}")))
        })?;
    info!(
        "ffmpeg spawned for normalized playback in {:?}: attempt_id={:?}",
        ffmpeg_started_at.elapsed(),
        attempt_id
    );

    drain_child_stderr(
        child.stderr.take(),
        MediaTool::Ffmpeg,
        attempt_id,
        "normalized-playback",
    );

    Ok(ChildContainer::from(vec![ytdlp_child, child]).into())
}
