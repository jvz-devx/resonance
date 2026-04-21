pub mod clear;
pub mod join;
pub mod leave;
pub mod loop_cmd;
pub mod normalize;
pub mod nowplaying;
pub mod pause;
pub mod play;
pub mod queue;
pub mod remove;
pub mod resume;
pub mod search;
pub mod shuffle;
pub mod skip;
pub mod stop;

use serenity::builder::{CreateCommand, CreateCommandOption};
use serenity::model::application::CommandOptionType;

/// Build all slash command definitions for global registration
pub fn all_commands() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new("play")
            .description("Play a song from YouTube (URL or search query)")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "query",
                    "YouTube URL or search query",
                )
                .required(true),
            ),
        CreateCommand::new("search")
            .description("Search YouTube and pick from 3 results")
            .add_option(
                CreateCommandOption::new(CommandOptionType::String, "query", "Search query")
                    .required(true),
            ),
        CreateCommand::new("queue").description("Show the current music queue"),
        CreateCommand::new("skip").description("Skip the current track"),
        CreateCommand::new("pause").description("Pause the current track"),
        CreateCommand::new("resume").description("Resume the paused track"),
        CreateCommand::new("stop").description("Stop playback and clear the queue"),
        CreateCommand::new("nowplaying").description("Show the currently playing track"),
        CreateCommand::new("shuffle").description("Shuffle the queue"),
        CreateCommand::new("loop")
            .description("Set the loop mode")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "mode",
                    "Loop mode: off, track, or queue",
                )
                .required(true)
                .add_string_choice("Off", "off")
                .add_string_choice("Track", "track")
                .add_string_choice("Queue", "queue"),
            ),
        CreateCommand::new("remove")
            .description("Remove a track from the queue by position")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::Integer,
                    "position",
                    "Position in queue (1-based)",
                )
                .required(true)
                .min_int_value(1),
            ),
        CreateCommand::new("clear")
            .description("Clear the queue (keeps the current track playing)"),
        CreateCommand::new("normalize")
            .description("Toggle sound normalisation (on by default)")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "enabled",
                    "Turn normalisation on or off (toggles if omitted)",
                )
                .add_string_choice("On", "on")
                .add_string_choice("Off", "off"),
            ),
        CreateCommand::new("join").description("Join your voice channel"),
        CreateCommand::new("leave").description("Leave the voice channel"),
    ]
}
