mod duration;
pub use duration::Duration;

pub mod factory;
pub use self::factory::Factory;

mod format;
pub use self::format::Reader;

pub mod media_info;
pub use self::media_info::{get_default_chapter_title, MediaInfo, Stream, Streams};

mod mkvmerge_text_format;
pub use self::mkvmerge_text_format::MKVMergeTextFormat;

mod timestamp_4_humans;
pub use self::timestamp_4_humans::{parse_timestamp, Timestamp4Humans};

mod toc_visitor;
pub use self::toc_visitor::{TocVisit, TocVisitor};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Format {
    MKVMergeText,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaContent {
    Audio,
    AudioVideo,
    AudioText,
    AudioVideoText,
    Text,
    Video,
    VideoText,
    Undefined,
}

impl MediaContent {
    pub fn add_stream_type(&mut self, type_: gst::StreamType) {
        match type_ {
            gst::StreamType::AUDIO => match self {
                MediaContent::Text => *self = MediaContent::AudioText,
                MediaContent::Video => *self = MediaContent::AudioVideo,
                MediaContent::VideoText => *self = MediaContent::AudioVideoText,
                MediaContent::Undefined => *self = MediaContent::Audio,
                _ => (),
            },
            gst::StreamType::VIDEO => match self {
                MediaContent::Audio => *self = MediaContent::AudioVideo,
                MediaContent::Text => *self = MediaContent::VideoText,
                MediaContent::AudioText => *self = MediaContent::AudioVideoText,
                MediaContent::Undefined => *self = MediaContent::Video,
                _ => (),
            },
            gst::StreamType::TEXT => match self {
                MediaContent::Audio => *self = MediaContent::AudioText,
                MediaContent::Video => *self = MediaContent::VideoText,
                MediaContent::AudioVideo => *self = MediaContent::AudioVideoText,
                MediaContent::Undefined => *self = MediaContent::Text,
                _ => (),
            },
            _ => panic!("MediaContent::add_stream_type can't handle {:?}", type_),
        };
    }
}

impl Default for MediaContent {
    fn default() -> Self {
        MediaContent::Undefined
    }
}

use nom::{character::complete::digit1, error::ErrorKind, Err, IResult};

fn parse_to<T: std::str::FromStr>(i: &str) -> IResult<&str, T> {
    let (i, res) = digit1(i)?;

    res.parse::<T>()
        .map(move |value| (i, value))
        .map_err(move |_| Err::Error((i, ErrorKind::ParseTo)))
}
