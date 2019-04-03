use gstreamer as gst;

use std::io::Read;

use super::MediaInfo;

pub trait Reader {
    fn read(&self, info: &MediaInfo, source: &mut dyn Read) -> Result<Option<gst::Toc>, String>;
}
