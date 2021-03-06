use gettextrs::gettext;
use gst::Tag;
use lazy_static::lazy_static;

use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use super::{Duration, MediaContent};

#[derive(Debug)]
pub struct SelectStreamError(Arc<str>);

impl SelectStreamError {
    fn new(id: &Arc<str>) -> Self {
        SelectStreamError(Arc::clone(&id))
    }

    pub fn id(&self) -> &Arc<str> {
        &self.0
    }
}

impl fmt::Display for SelectStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MediaInfo: unknown stream id {}", self.0)
    }
}
impl std::error::Error for SelectStreamError {}

pub fn get_default_chapter_title() -> String {
    gettext("untitled")
}

macro_rules! add_tag_names (
    ($($tag_type:path),+) => {
        {
            let mut tag_names = Vec::new();
            $(tag_names.push(<$tag_type>::tag_name());)+
            tag_names
        }
    };
);

lazy_static! {
    static ref TAGS_TO_SKIP_FOR_TRACK: Vec<&'static str> = {
        add_tag_names!(
            gst::tags::Album,
            gst::tags::AlbumSortname,
            gst::tags::AlbumSortname,
            gst::tags::AlbumArtist,
            gst::tags::AlbumArtistSortname,
            gst::tags::ApplicationName,
            gst::tags::ApplicationData,
            gst::tags::Artist,
            gst::tags::ArtistSortname,
            gst::tags::AudioCodec,
            gst::tags::Codec,
            gst::tags::ContainerFormat,
            gst::tags::Duration,
            gst::tags::Encoder,
            gst::tags::EncoderVersion,
            gst::tags::Image,
            gst::tags::ImageOrientation,
            gst::tags::PreviewImage,
            gst::tags::SubtitleCodec,
            gst::tags::Title,
            gst::tags::TitleSortname,
            gst::tags::TrackCount,
            gst::tags::TrackNumber,
            gst::tags::VideoCodec
        )
    };
}

#[derive(Debug, Clone)]
pub struct Stream {
    pub id: Arc<str>,
    pub codec_printable: String,
    pub caps: gst::Caps,
    pub tags: gst::TagList,
    pub type_: gst::StreamType,
}

impl Stream {
    fn new(stream: &gst::Stream) -> Self {
        let caps = stream.get_caps().unwrap();
        let tags = stream.get_tags().unwrap_or_else(gst::TagList::new);
        let type_ = stream.get_stream_type();

        let codec_printable = match type_ {
            gst::StreamType::AUDIO => tags.get_index::<gst::tags::AudioCodec>(0),
            gst::StreamType::VIDEO => tags.get_index::<gst::tags::VideoCodec>(0),
            gst::StreamType::TEXT => tags.get_index::<gst::tags::SubtitleCodec>(0),
            _ => panic!("Stream::new can't handle {:?}", type_),
        }
        .or_else(|| tags.get_index::<gst::tags::Codec>(0))
        .and_then(|value| value.get())
        .map_or_else(
            || {
                // codec in caps in the form "streamtype/x-codec"
                let codec = caps.get_structure(0).unwrap().get_name();
                let id_parts: Vec<&str> = codec.split('/').collect();
                if id_parts.len() == 2 {
                    if id_parts[1].starts_with("x-") {
                        id_parts[1][2..].to_string()
                    } else {
                        id_parts[1].to_string()
                    }
                } else {
                    codec.to_string()
                }
            },
            ToString::to_string,
        );

        Stream {
            id: stream.get_stream_id().unwrap().as_str().into(),
            codec_printable,
            caps,
            tags,
            type_,
        }
    }
}

#[derive(Debug)]
pub struct StreamCollection {
    type_: gst::StreamType,
    collection: HashMap<Arc<str>, Stream>,
}

impl StreamCollection {
    fn new(type_: gst::StreamType) -> Self {
        StreamCollection {
            type_,
            collection: HashMap::new(),
        }
    }

    fn add_stream(&mut self, stream: Stream) {
        self.collection.insert(Arc::clone(&stream.id), stream);
    }

    pub fn get<S: AsRef<str>>(&self, id: S) -> Option<&Stream> {
        self.collection.get(id.as_ref())
    }

    pub fn contains<S: AsRef<str>>(&self, id: S) -> bool {
        self.collection.contains_key(id.as_ref())
    }

    pub fn sorted(&self) -> impl Iterator<Item = &'_ Stream> {
        SortedStreamCollectionIter::new(self)
    }
}

struct SortedStreamCollectionIter<'sc> {
    collection: &'sc StreamCollection,
    sorted_iter: std::vec::IntoIter<Arc<str>>,
}

impl<'sc> SortedStreamCollectionIter<'sc> {
    fn new(collection: &'sc StreamCollection) -> Self {
        let mut sorted_ids: Vec<Arc<str>> = collection.collection.keys().map(Arc::clone).collect();
        sorted_ids.sort();

        SortedStreamCollectionIter {
            collection,
            sorted_iter: sorted_ids.into_iter(),
        }
    }
}

impl<'sc> Iterator for SortedStreamCollectionIter<'sc> {
    type Item = &'sc Stream;

    fn next(&mut self) -> Option<Self::Item> {
        self.sorted_iter
            .next()
            .and_then(|id| self.collection.get(&id))
    }
}

#[derive(Debug)]
pub struct Streams {
    pub audio: StreamCollection,
    pub video: StreamCollection,
    pub text: StreamCollection,

    cur_audio_id: Option<Arc<str>>,
    pub audio_changed: bool,
    cur_video_id: Option<Arc<str>>,
    pub video_changed: bool,
    cur_text_id: Option<Arc<str>>,
    pub text_changed: bool,
}

impl Default for Streams {
    fn default() -> Self {
        Streams {
            audio: StreamCollection::new(gst::StreamType::AUDIO),
            video: StreamCollection::new(gst::StreamType::VIDEO),
            text: StreamCollection::new(gst::StreamType::TEXT),

            cur_audio_id: None,
            audio_changed: false,
            cur_video_id: None,
            video_changed: false,
            cur_text_id: None,
            text_changed: false,
        }
    }
}

impl Streams {
    pub fn add_stream(&mut self, gst_stream: &gst::Stream) {
        let stream = Stream::new(gst_stream);
        match stream.type_ {
            gst::StreamType::AUDIO => {
                self.cur_audio_id.get_or_insert(Arc::clone(&stream.id));
                self.audio.add_stream(stream);
            }
            gst::StreamType::VIDEO => {
                self.cur_video_id.get_or_insert(Arc::clone(&stream.id));
                self.video.add_stream(stream);
            }
            gst::StreamType::TEXT => {
                self.cur_text_id.get_or_insert(Arc::clone(&stream.id));
                self.text.add_stream(stream);
            }
            other => unimplemented!("{:?}", other),
        }
    }

    pub fn collection(&self, type_: gst::StreamType) -> &StreamCollection {
        match type_ {
            gst::StreamType::AUDIO => &self.audio,
            gst::StreamType::VIDEO => &self.video,
            gst::StreamType::TEXT => &self.text,
            other => unimplemented!("{:?}", other),
        }
    }

    pub fn is_video_selected(&self) -> bool {
        self.cur_video_id.is_some()
    }

    pub fn selected_audio(&self) -> Option<&Stream> {
        self.cur_audio_id
            .as_ref()
            .and_then(|stream_id| self.audio.get(stream_id))
    }

    pub fn selected_video(&self) -> Option<&Stream> {
        self.cur_video_id
            .as_ref()
            .and_then(|stream_id| self.video.get(stream_id))
    }

    pub fn selected_text(&self) -> Option<&Stream> {
        self.cur_text_id
            .as_ref()
            .and_then(|stream_id| self.text.get(stream_id))
    }

    pub fn select_streams(&mut self, ids: &[Arc<str>]) -> Result<(), SelectStreamError> {
        let mut is_audio_selected = false;
        let mut is_text_selected = false;
        let mut is_video_selected = false;

        for id in ids {
            if self.audio.contains(id) {
                is_audio_selected = true;
                self.audio_changed = self
                    .selected_audio()
                    .map_or(true, |prev_stream| *id != prev_stream.id);
                self.cur_audio_id = Some(Arc::clone(id));
            } else if self.text.contains(id) {
                is_text_selected = true;
                self.text_changed = self
                    .selected_text()
                    .map_or(true, |prev_stream| *id != prev_stream.id);
                self.cur_text_id = Some(Arc::clone(id));
            } else if self.video.contains(id) {
                is_video_selected = true;
                self.video_changed = self
                    .selected_video()
                    .map_or(true, |prev_stream| *id != prev_stream.id);
                self.cur_video_id = Some(Arc::clone(id));
            } else {
                return Err(SelectStreamError::new(id));
            }
        }

        if !is_audio_selected {
            self.audio_changed = self.cur_audio_id.take().map_or(false, |_| true);
        }
        if !is_text_selected {
            self.text_changed = self.cur_text_id.take().map_or(false, |_| true);
        }
        if !is_video_selected {
            self.video_changed = self.cur_video_id.take().map_or(false, |_| true);
        }

        Ok(())
    }

    pub fn audio_codec(&self) -> Option<&str> {
        self.selected_audio()
            .map(|stream| stream.codec_printable.as_str())
    }

    pub fn video_codec(&self) -> Option<&str> {
        self.selected_video()
            .map(|stream| stream.codec_printable.as_str())
    }

    fn tag_list<'a, T: gst::Tag<'a>>(&'a self) -> Option<&gst::TagList> {
        self.selected_audio()
            .and_then(|selected_audio| {
                if selected_audio.tags.get_size::<T>() > 0 {
                    Some(&selected_audio.tags)
                } else {
                    None
                }
            })
            .or_else(|| {
                self.selected_video().and_then(|selected_video| {
                    if selected_video.tags.get_size::<T>() > 0 {
                        Some(&selected_video.tags)
                    } else {
                        None
                    }
                })
            })
    }
}

#[derive(Debug, Default)]
pub struct MediaInfo {
    pub name: String,
    pub file_name: String,
    pub path: PathBuf,
    pub content: MediaContent,
    pub tags: gst::TagList,
    pub toc: Option<gst::Toc>,
    pub chapter_count: Option<usize>,

    pub description: String,
    pub duration: Duration,

    pub streams: Streams,
}

impl MediaInfo {
    pub fn new(path: &Path) -> Self {
        MediaInfo {
            name: path.file_stem().unwrap().to_str().unwrap().to_owned(),
            file_name: path.file_name().unwrap().to_str().unwrap().to_owned(),
            path: path.to_owned(),
            ..MediaInfo::default()
        }
    }

    pub fn add_stream(&mut self, gst_stream: &gst::Stream) {
        self.streams.add_stream(gst_stream);
        self.content.add_stream_type(gst_stream.get_stream_type());
    }

    pub fn add_tags(&mut self, tags: &gst::TagList) {
        self.tags = self.tags.merge(tags, gst::TagMergeMode::Keep);
    }

    fn tag_list<'a, T: gst::Tag<'a>>(&'a self) -> Option<&gst::TagList> {
        if self.tags.get_size::<T>() > 0 {
            Some(&self.tags)
        } else {
            None
        }
    }

    fn tag_for_display<'a, Primary, Secondary>(
        &'a self,
    ) -> Option<<Primary as gst::Tag<'a>>::TagType>
    where
        Primary: gst::Tag<'a> + 'a,
        Secondary: gst::Tag<'a, TagType = <Primary as gst::Tag<'a>>::TagType> + 'a,
    {
        self.tag_list::<Primary>()
            .or_else(|| self.tag_list::<Secondary>())
            .or_else(|| {
                self.streams
                    .tag_list::<Primary>()
                    .or_else(|| self.streams.tag_list::<Secondary>())
            })
            .and_then(|tag_list| {
                tag_list
                    .get_index::<Primary>(0)
                    .or_else(|| tag_list.get_index::<Secondary>(0))
                    .and_then(|value| value.get())
            })
    }

    pub fn media_artist(&self) -> Option<&str> {
        self.tag_for_display::<gst::tags::Artist, gst::tags::AlbumArtist>()
    }

    pub fn media_title(&self) -> Option<&str> {
        self.tag_for_display::<gst::tags::Title, gst::tags::Album>()
    }

    pub fn media_image(&self) -> Option<gst::Sample> {
        self.tag_for_display::<gst::tags::Image, gst::tags::PreviewImage>()
    }

    pub fn container(&self) -> Option<&str> {
        // in case of an mp3 audio file, container comes as `ID3 label`
        // => bypass it
        if let Some(audio_codec) = self.streams.audio_codec() {
            if self.streams.video_codec().is_none()
                && audio_codec.to_lowercase().find("mp3").is_some()
            {
                return None;
            }
        }

        self.tags
            .get_index::<gst::tags::ContainerFormat>(0)
            .and_then(|value| value.get())
    }
}
