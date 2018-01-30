extern crate gstreamer as gst;

use std::io::Read;

use super::{Chapter, MediaInfo};

pub trait Reader {
    fn read(&self, info: &MediaInfo, duration: u64, source: &mut Read, chapters: &mut Vec<Chapter>);
}
