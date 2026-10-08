use thiserror::Error;

/// Every error the game raises. Foreign errors (IO, serde) are wrapped, never leaked raw.
#[derive(Debug, Error)]
pub enum GameError {
    /// Content failed to parse or validate. The message names the file and the valid set.
    #[error("content error: {0}")]
    Content(String),
    /// A save could not be read, written or migrated.
    #[error("save error: {0}")]
    Save(String),
    /// A caller broke a documented precondition.
    #[error("contract violation: {0}")]
    Contract(String),
    /// The world reached a state its invariants forbid.
    #[error("invariant violation: {0}")]
    Invariant(String),
}

impl GameError {
    pub fn content(msg: impl Into<String>) -> Self {
        GameError::Content(msg.into())
    }
    pub fn save(msg: impl Into<String>) -> Self {
        GameError::Save(msg.into())
    }
}
