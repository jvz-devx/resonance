use songbird::input::{ChildContainer, Input};
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::debug;

use crate::utils::error::{BotError, BotResult};

/// Create an audio source that pipes through ffmpeg with dynamic audio normalization.
///
/// Resolves the direct audio URL via yt-dlp, then spawns ffmpeg to decode and
/// normalize the audio, outputting Opus-in-Ogg on stdout. Outputting Opus lets
/// Songbird pass the encoded frames straight to Discord without a second encode.
pub async fn create_normalized_source(url: &str) -> BotResult<Input> {
    // Resolve direct stream URL via yt-dlp
    debug!("Resolving URL via yt-dlp for normalization: {url}");
    let ytdl_future = Command::new("yt-dlp")
        .args([
            "-j",
            "-f",
            "ba/b[vcodec=none]/b",
            "--no-playlist",
            url,
        ])
        .output();
    let ytdl_output = timeout(Duration::from_secs(15), ytdl_future)
        .await
        .map_err(|_| BotError::Other("yt-dlp timed out after 15s".into()))?
        .map_err(|e| BotError::Other(format!("Failed to run yt-dlp: {e}")))?;

    if !ytdl_output.status.success() {
        let stderr = String::from_utf8_lossy(&ytdl_output.stderr);
        return Err(BotError::Other(format!("yt-dlp failed: {stderr}")));
    }

    let info: serde_json::Value = serde_json::from_slice(&ytdl_output.stdout)
        .map_err(|e| BotError::Other(format!("Failed to parse yt-dlp output: {e}")))?;

    let stream_url = info["url"]
        .as_str()
        .ok_or_else(|| BotError::Other("yt-dlp output missing 'url' field".to_string()))?;

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

    debug!("Spawning ffmpeg with dynaudnorm + opus/ogg for normalized playback");
    let mut child = std::process::Command::new("ffmpeg")
        .args(&ffmpeg_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| BotError::Other(format!("Failed to spawn ffmpeg: {e}")))?;

    // Drain stderr on a background task so expired URLs / 403s / hangs are visible.
    if let Some(stderr) = child.stderr.take() {
        tokio::task::spawn_blocking(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                debug!(target: "ffmpeg", "{line}");
            }
        });
    }

    Ok(ChildContainer::from(child).into())
}
