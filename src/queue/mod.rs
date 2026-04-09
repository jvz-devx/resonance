pub mod track;

use std::collections::VecDeque;

use rand::seq::SliceRandom;

use crate::queue::track::TrackMetadata;

/// Manages the track queue for a guild.
/// This is the pure data structure — no songbird or Discord logic here.
#[derive(Debug, Default)]
pub struct QueueManager {
    pub tracks: VecDeque<TrackMetadata>,
}

impl QueueManager {
    pub fn new() -> Self {
        Self {
            tracks: VecDeque::new(),
        }
    }

    #[allow(dead_code)]
    /// Reconstruct from a persisted list of tracks
    pub fn from_tracks(tracks: Vec<TrackMetadata>) -> Self {
        Self {
            tracks: VecDeque::from(tracks),
        }
    }

    /// Add a track to the end of the queue. Returns queue position (1-indexed).
    pub fn enqueue(&mut self, track: TrackMetadata) -> usize {
        self.tracks.push_back(track);
        self.tracks.len()
    }

    /// Remove and return the next track from the front.
    pub fn dequeue(&mut self) -> Option<TrackMetadata> {
        self.tracks.pop_front()
    }

    #[allow(dead_code)]
    /// Peek at the next track without removing it.
    pub fn peek(&self) -> Option<&TrackMetadata> {
        self.tracks.front()
    }

    /// Remove a track at a specific 1-indexed position. Returns the removed track.
    pub fn remove(&mut self, position: usize) -> Option<TrackMetadata> {
        if position == 0 || position > self.tracks.len() {
            return None;
        }
        self.tracks.remove(position - 1)
    }

    /// Shuffle the queue in place.
    pub fn shuffle(&mut self) {
        let mut vec: Vec<TrackMetadata> = self.tracks.drain(..).collect();
        let mut rng = rand::rng();
        vec.shuffle(&mut rng);
        self.tracks = VecDeque::from(vec);
    }

    /// Clear the entire queue.
    pub fn clear(&mut self) {
        self.tracks.clear();
    }

    /// Number of tracks in the queue.
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    #[allow(dead_code)]
    /// Whether the queue is empty. Paired with `len()` to satisfy clippy's
    /// `len_without_is_empty` lint; kept public for future use.
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    #[allow(dead_code)]
    /// Get a slice-like view of the queue for display.
    pub fn as_slice(&self) -> Vec<&TrackMetadata> {
        self.tracks.iter().collect()
    }
}
