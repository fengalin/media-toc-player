pub mod context;
pub use self::context::PlaybackContext;

pub enum ContextMessage {
    AsyncDone,
    Eos,
    FailedToOpenMedia(String),
    InitDone,
    StreamsSelected,
}
