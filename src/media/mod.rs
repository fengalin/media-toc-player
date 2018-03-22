use glib;

pub mod context;
pub use self::context::Context;

pub enum ContextMessage {
    AsyncDone,
    Eos,
    FailedToOpenMedia(glib::Error),
    InitDone,
    StreamsSelected,
}
