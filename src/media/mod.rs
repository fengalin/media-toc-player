pub mod playback_pipeline;
pub use self::playback_pipeline::PlaybackPipeline;

pub mod timestamp;
pub use self::timestamp::Timestamp;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlaybackState {
    Paused,
    Playing,
}

#[derive(Clone, Debug)]
pub enum MediaEvent {
    AsyncDone(PlaybackState),
    Eos,
    FailedToOpenMedia(String),
    GLSinkError,
    InitDone,
    MissingPlugin(String),
    ReadyToRefresh,
    StreamsSelected,
}
