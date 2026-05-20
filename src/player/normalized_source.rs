use songbird::input::{ChildContainer, Input};
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::player::media_tools::{build_ytdlp_user_args, classify_media_error, log_excerpt};
use crate::utils::error::{BotError, BotResult};

/// Create an audio source that pipes through ffmpeg with dynamic audio normalization.
///
/// Resolves the direct audio URL via yt-dlp, then spawns ffmpeg to decode and
/// normalize the audio, outputting Opus-in-Ogg on stdout. Outputting Opus lets
/// Songbird pass the encoded frames straight to Discord without a second encode.
pub async fn create_normalized_source_with_timeout(
    url: &str,
    ytdl_timeout: Duration,
    attempt_id: Option<u64>,
) -> BotResult<Input> {
    // Resolve direct stream URL via yt-dlp
    let ytdl_started_at = Instant::now();
    info!(
        "Resolving URL via yt-dlp for normalized playback: attempt_id={:?}, timeout={:?}, url={url}",
        attempt_id, ytdl_timeout
    );
    let mut ytdl_command = Command::new("yt-dlp");
    ytdl_command.args(build_ytdlp_user_args());
    ytdl_command.args([
        "-j",
        "-f",
        "ba[abr>0][vcodec=none]/best",
        "--no-playlist",
        url,
    ]);

    let ytdl_output = timeout(ytdl_timeout, ytdl_command.output())
        .await
        .map_err(|_| {
            warn!(
                "yt-dlp timed out while preparing normalized playback: attempt_id={:?}, timeout={:?}, url={url}",
                attempt_id, ytdl_timeout
            );
            BotError::ExtractorFailed(format!("yt-dlp timed out after {}s", ytdl_timeout.as_secs()))
        })?
        .map_err(|e| {
            warn!(
                "Failed to run yt-dlp for normalized playback after {:?}: attempt_id={:?}, error={e}",
                ytdl_started_at.elapsed(),
                attempt_id
            );
            BotError::ExtractorFailed(log_excerpt(&format!("Failed to run yt-dlp: {e}")))
        })?;

    if !ytdl_output.status.success() {
        let stderr = String::from_utf8_lossy(&ytdl_output.stderr);
        warn!(
            "yt-dlp failed for normalized playback after {:?}: attempt_id={:?}, {}",
            ytdl_started_at.elapsed(),
            attempt_id,
            log_excerpt(stderr.as_ref())
        );
        return Err(classify_media_error(stderr.as_ref()));
    }

    info!(
        "yt-dlp resolved normalized playback source in {:?}: attempt_id={:?}",
        ytdl_started_at.elapsed(),
        attempt_id
    );

    let info: serde_json::Value = serde_json::from_slice(&ytdl_output.stdout).map_err(|e| {
        BotError::ExtractorFailed(log_excerpt(&format!("Failed to parse yt-dlp output: {e}")))
    })?;

    let stream_url = info["url"].as_str().ok_or_else(|| {
        BotError::ExtractorFailed("yt-dlp output missing 'url' field".to_string())
    })?;
    let format_id = info["format_id"].as_str().unwrap_or("unknown");
    let protocol = info["protocol"].as_str().unwrap_or("unknown");
    let abr = info["abr"]
        .as_f64()
        .map(|v| format!("{v:.0}kbps"))
        .unwrap_or_else(|| "unknown".to_string());
    info!(
        "Normalized source metadata: attempt_id={:?}, format_id={format_id}, protocol={protocol}, abr={abr}",
        attempt_id
    );

    // Build a SINGLE -headers string (ffmpeg only honors the last one).
    // Headers must come BEFORE -i. Same for -reconnect flags.
    //
    // Googlevideo CDN drops long-lived TLS connections mid-stream with
    // "Connection reset by peer"; without these flags ffmpeg surfaces an
    // I/O error and the track ends early. Notes:
    // - reconnect_on_network_error handles TCP/TLS errors (our case)
    // - reconnect_streamed is required for non-seekable HTTP streams
    // - reconnect_at_eof is intentionally OMITTED because it would loop
    //   forever at the natural end of a finite track
    let mut ffmpeg_args: Vec<String> = vec![
        "-nostdin".to_string(),
        "-reconnect".to_string(),
        "1".to_string(),
        "-reconnect_streamed".to_string(),
        "1".to_string(),
        "-reconnect_on_network_error".to_string(),
        "1".to_string(),
        "-reconnect_delay_max".to_string(),
        "5".to_string(),
    ];

    if let Some(headers) = info["http_headers"].as_object() {
        let joined: String = headers
            .iter()
            .filter_map(|(k, v)| v.as_str().map(|val| format!("{k}: {val}")))
            .collect::<Vec<_>>()
            .join("\r\n");
        if !joined.is_empty() {
            ffmpeg_args.push("-headers".to_string());
            // Trailing CRLF is customary for ffmpeg's -headers
            ffmpeg_args.push(format!("{joined}\r\n"));
        }
    }

    ffmpeg_args.extend([
        "-i".to_string(),
        stream_url.to_string(),
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
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
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

    // Drain stderr on a background task so expired URLs / 403s / hangs are visible.
    if let Some(stderr) = child.stderr.take() {
        tokio::task::spawn_blocking(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                match classify_media_error(&line) {
                    BotError::AntiBotChallenge
                    | BotError::RateLimited
                    | BotError::MediaForbidden
                    | BotError::StreamNetwork(_) => {
                        warn!(target: "ffmpeg", "attempt_id={attempt_id:?} {line}")
                    }
                    _ if is_ffmpeg_diagnostic_line(&line) => {
                        warn!(target: "ffmpeg", "attempt_id={attempt_id:?} {line}")
                    }
                    _ => debug!(target: "ffmpeg", "attempt_id={attempt_id:?} {line}"),
                }
            }
        });
    }

    Ok(ChildContainer::from(child).into())
}

fn is_ffmpeg_diagnostic_line(line: &str) -> bool {
    let line = line.to_lowercase();
    [
        "buffer",
        "delay",
        "drop",
        "error",
        "http",
        "i/o",
        "non-monotonous",
        "reconnect",
        "reset",
        "timeout",
        "underrun",
    ]
    .iter()
    .any(|needle| line.contains(needle))
}
