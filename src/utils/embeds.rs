use serenity::builder::CreateEmbed;

use crate::queue::track::TrackMetadata;
use crate::state::LoopMode;

const COLOR_PRIMARY: u32 = 0xFF0000; // YouTube red
const COLOR_SUCCESS: u32 = 0x00FF00;
const COLOR_ERROR: u32 = 0xFF4444;
const COLOR_INFO: u32 = 0x5865F2; // Discord blurple
const MAX_DESCRIPTION_LEN: usize = 4000;

fn escape_markdown(s: &str) -> String {
    s.replace('[', "\\[")
        .replace(']', "\\]")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn truncate_description(s: &mut String) {
    if s.len() > MAX_DESCRIPTION_LEN {
        // Find a valid UTF-8 char boundary to avoid panicking on multi-byte chars
        let mut end = MAX_DESCRIPTION_LEN;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        s.truncate(end);
        s.push_str("...");
    }
}

pub fn now_playing_embed(track: &TrackMetadata) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .title("Now Playing")
        .description(format!(
            "**[{}]({})**",
            escape_markdown(&track.title),
            track.url
        ))
        .color(COLOR_PRIMARY)
        .field(
            "Duration",
            track
                .duration
                .map(|d| format_duration(d))
                .unwrap_or_else(|| "Live".into()),
            true,
        )
        .field("Requested by", &track.requester_name, true);

    if let Some(ref thumb) = track.thumbnail {
        embed = embed.thumbnail(thumb);
    }

    embed
}

pub fn added_to_queue_embed(track: &TrackMetadata, position: usize) -> CreateEmbed {
    let mut embed = CreateEmbed::new()
        .title("Added to Queue")
        .description(format!(
            "**[{}]({})**",
            escape_markdown(&track.title),
            track.url
        ))
        .color(COLOR_SUCCESS)
        .field("Position", format!("#{position}"), true)
        .field(
            "Duration",
            track
                .duration
                .map(|d| format_duration(d))
                .unwrap_or_else(|| "Live".into()),
            true,
        );

    if let Some(ref thumb) = track.thumbnail {
        embed = embed.thumbnail(thumb);
    }

    embed
}

pub fn queue_embed(
    now_playing: Option<&TrackMetadata>,
    queue: &[TrackMetadata],
    loop_mode: &LoopMode,
) -> CreateEmbed {
    let mut description = String::new();

    if let Some(np) = now_playing {
        description.push_str(&format!(
            "**Now Playing:**\n[{}]({}) | `{}`\n\n",
            escape_markdown(&np.title),
            np.url,
            np.duration
                .map(|d| format_duration(d))
                .unwrap_or_else(|| "Live".into())
        ));
    }

    if queue.is_empty() {
        description.push_str("*Queue is empty*");
    } else {
        description.push_str("**Up Next:**\n");
        for (i, track) in queue.iter().enumerate().take(10) {
            description.push_str(&format!(
                "`{}.` [{}]({}) | `{}`\n",
                i + 1,
                escape_markdown(&track.title),
                track.url,
                track
                    .duration
                    .map(|d| format_duration(d))
                    .unwrap_or_else(|| "Live".into())
            ));
        }
        if queue.len() > 10 {
            description.push_str(&format!("\n*...and {} more tracks*", queue.len() - 10));
        }
    }

    truncate_description(&mut description);

    let loop_str = match loop_mode {
        LoopMode::Off => "Off",
        LoopMode::Track => "Track",
        LoopMode::Queue => "Queue",
    };

    CreateEmbed::new()
        .title("Music Queue")
        .description(description)
        .color(COLOR_INFO)
        .footer(serenity::builder::CreateEmbedFooter::new(format!(
            "{} tracks in queue | Loop: {loop_str}",
            queue.len()
        )))
}

pub fn search_results_embed(
    results: &[(String, String, Option<std::time::Duration>)],
) -> CreateEmbed {
    let mut description = String::new();
    let emojis = [
        "1\u{fe0f}\u{20e3}",
        "2\u{fe0f}\u{20e3}",
        "3\u{fe0f}\u{20e3}",
    ];

    for (i, (title, _url, duration)) in results.iter().enumerate() {
        let dur_str = duration
            .map(|d| format_duration(d))
            .unwrap_or_else(|| "Live".into());
        description.push_str(&format!("{} **{}** | `{}`\n\n", emojis[i], title, dur_str));
    }

    description.push_str("\n*React with a number to select a track. Expires in 30 seconds.*");

    truncate_description(&mut description);

    CreateEmbed::new()
        .title("Search Results")
        .description(description)
        .color(COLOR_INFO)
}

pub fn error_embed(message: &str) -> CreateEmbed {
    CreateEmbed::new()
        .title("Error")
        .description(message)
        .color(COLOR_ERROR)
}

pub fn success_embed(title: &str, description: &str) -> CreateEmbed {
    CreateEmbed::new()
        .title(title)
        .description(description)
        .color(COLOR_SUCCESS)
}

pub fn format_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}
