extern crate gstreamer as gst;

use std::io::Read;

use super::MediaInfo;

pub trait Reader {
    fn read(&self, info: &MediaInfo, source: &mut Read) -> Option<gst::Toc>;
}
