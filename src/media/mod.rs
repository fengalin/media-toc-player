pub mod playback_pipeline;
pub use self::playback_pipeline::PlaybackPipeline;

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
    InitDone,
    MissingPlugin(String),
    ReadyForRefresh,
    StreamsSelected,
}
