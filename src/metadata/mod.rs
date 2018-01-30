pub mod chapter;
pub use self::chapter::Chapter;

pub mod factory;
pub use self::factory::Factory;

pub mod format;
pub use self::format::Reader;

pub mod media_info;
pub use self::media_info::{MediaInfo, Stream, Streams};

pub mod mkvmerge_text_format;
pub use self::mkvmerge_text_format::MKVMergeTextFormat;

pub mod timestamp;
pub use self::timestamp::Timestamp;

pub static METADATA_TITLE: &'static str = "title";

#[derive(Clone, Debug, PartialEq)]
pub enum Format {
    MKVMergeText,
}
