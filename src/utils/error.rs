#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum BotError {
    #[error("You must be in a voice channel")]
    NotInVoice,
    #[error("Not in a guild")]
    NotInGuild,
    #[error("Nothing is currently playing")]
    NothingPlaying,
    #[error("Queue is empty")]
    QueueEmpty,
    #[error("Invalid position: {0}")]
    InvalidPosition(usize),
    #[error("YouTube search failed: {0}")]
    SearchFailed(String),
    #[error("No search results found")]
    NoResults,
    #[error("Failed to play audio: {0}")]
    PlayFailed(String),
    #[error("Internal state missing: {0}")]
    StateMissing(String),
    #[error("Redis error: {0}")]
    Redis(#[from] deadpool_redis::redis::RedisError),
    #[error("Redis pool error: {0}")]
    RedisPool(#[from] deadpool_redis::PoolError),
    #[error("Serenity error: {0}")]
    Serenity(#[from] serenity::Error),
    #[error("Songbird join error: {0}")]
    JoinError(String),
    #[error("{0}")]
    Other(String),
}

/// Convenience type alias
pub type BotResult<T> = Result<T, BotError>;

impl BotError {
    /// Returns a user-friendly short message for embed display
    pub fn user_message(&self) -> String {
        match self {
            Self::NotInVoice => "You need to be in a voice channel to use this command.".into(),
            Self::NotInGuild => "This command can only be used in a server.".into(),
            Self::NothingPlaying => "Nothing is currently playing.".into(),
            Self::QueueEmpty => "The queue is empty.".into(),
            Self::InvalidPosition(pos) => format!("Invalid queue position: {pos}"),
            Self::SearchFailed(e) => format!("YouTube search failed: {e}"),
            Self::NoResults => "No results found for your search.".into(),
            Self::PlayFailed(e) => format!("Failed to play: {e}"),
            _ => format!("An error occurred: {self}"),
        }
    }
}
