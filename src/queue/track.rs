use std::time::Duration;

use serde::{Deserialize, Serialize};
use serenity::model::id::UserId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackMetadata {
    pub title: String,
    pub url: String,
    #[serde(with = "option_duration_millis")]
    pub duration: Option<Duration>,
    pub thumbnail: Option<String>,
    pub requester_id: u64,
    pub requester_name: String,
}

impl TrackMetadata {
    pub fn new(
        title: String,
        url: String,
        duration: Option<Duration>,
        thumbnail: Option<String>,
        requester_id: UserId,
        requester_name: String,
    ) -> Self {
        Self {
            title,
            url,
            duration,
            thumbnail,
            requester_id: requester_id.get(),
            requester_name,
        }
    }
}

/// Serialize Option<Duration> as Option<u64> millis for JSON compatibility
mod option_duration_millis {
    #[allow(unused_imports)]
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(d) => serializer.serialize_some(&d.as_millis()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<u64> = Option::deserialize(deserializer)?;
        Ok(opt.map(Duration::from_millis))
    }
}
