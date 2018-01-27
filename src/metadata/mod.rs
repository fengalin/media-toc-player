pub mod chapter;
pub use self::chapter::Chapter;

pub mod media_info;
pub use self::media_info::{MediaInfo, Stream, Streams};

pub mod timestamp;
pub use self::timestamp::Timestamp;

pub static METADATA_TITLE: &'static str = "title";
