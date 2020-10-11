use futures::channel::mpsc as async_chan;
use futures::prelude::*;

use gettextrs::gettext;

use gst::prelude::*;
use gst::ClockTime;
use gstreamer as gst;

use log::{error, info, warn};

use std::{collections::HashSet, convert::AsRef, fmt, path::Path, sync::Arc};

use crate::metadata::{media_info, Duration, MediaInfo};

use super::Timestamp;

#[derive(Debug)]
pub enum MediaMessage {
    Eos,
    Error(String),
}

#[derive(Debug)]
pub enum OpenError {
    GLSinkError,
    Generic(String),
    MissingPlugins(HashSet<String>),
    StateChange,
}

// TODO impl Error for OpenError

#[derive(Debug)]
struct PurgeError;

#[derive(Debug)]
pub struct StateChangeError;

impl From<gst::StateChangeError> for StateChangeError {
    fn from(_: gst::StateChangeError) -> Self {
        StateChangeError
    }
}

impl From<PurgeError> for StateChangeError {
    fn from(_: PurgeError) -> Self {
        StateChangeError
    }
}

impl fmt::Display for StateChangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Media: couldn't change state")
    }
}
impl std::error::Error for StateChangeError {}

#[derive(Debug)]
pub enum SeekError {
    Eos,
    Unrecoverable,
}

impl From<PurgeError> for SeekError {
    fn from(_: PurgeError) -> Self {
        SeekError::Unrecoverable
    }
}

impl fmt::Display for SeekError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SeekError::Eos => write!(f, "Media: seeking past the end"),
            SeekError::Unrecoverable => write!(f, "Media: couldn't seek"),
        }
    }
}
impl std::error::Error for SeekError {}

#[derive(Debug)]
pub enum SelectStreamsError {
    UnknownId(Arc<str>),
    Unrecoverable,
}

impl From<media_info::SelectStreamError> for SelectStreamsError {
    fn from(err: media_info::SelectStreamError) -> Self {
        SelectStreamsError::UnknownId(Arc::clone(err.id()))
    }
}

impl From<PurgeError> for SelectStreamsError {
    fn from(_: PurgeError) -> Self {
        SelectStreamsError::Unrecoverable
    }
}

impl fmt::Display for SelectStreamsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectStreamsError::UnknownId(id) => {
                write!(f, "Media: select stream: unknown id {}", id.as_ref())
            }
            SelectStreamsError::Unrecoverable => write!(f, "Media: couldn't select stream"),
        }
    }
}
impl std::error::Error for SelectStreamsError {}

pub struct PlaybackPipeline {
    pipeline: gst::Pipeline,
    pub info: MediaInfo,
    pub media_msg_rx: Option<async_chan::UnboundedReceiver<MediaMessage>>,
    int_msg_rx: async_chan::UnboundedReceiver<gst::Message>,
    bus_watch_src_id: Option<glib::SourceId>,
}

// FIXME: might need to `release_request_pad` on the tee
impl Drop for PlaybackPipeline {
    fn drop(&mut self) {
        if let Some(video_sink) = self.pipeline.get_by_name("video_sink") {
            self.pipeline.remove(&video_sink).unwrap();
        }
    }
}

/// Initialization
impl PlaybackPipeline {
    pub async fn try_new(
        path: &Path,
        video_sink: &Option<gst::Element>,
    ) -> Result<PlaybackPipeline, OpenError> {
        info!(
            "{}",
            gettext("Opening {}...").replacen("{}", path.to_str().unwrap(), 1)
        );

        let (ext_msg_tx, ext_msg_rx) = async_chan::unbounded();
        let (int_msg_tx, int_msg_rx) = async_chan::unbounded();

        let mut this = PlaybackPipeline {
            pipeline: gst::Pipeline::new(Some("playback_pipeline")),
            info: MediaInfo::new(path),
            media_msg_rx: Some(ext_msg_rx),
            int_msg_rx,
            bus_watch_src_id: None,
        };

        this.build_pipeline(path, video_sink);

        this.open().await?;
        this.register_bus_watch(ext_msg_tx, int_msg_tx);

        Ok(this)
    }

    pub fn check_requirements() -> Result<(), String> {
        gst::ElementFactory::make("decodebin3", None)
            .map(drop)
            .map_err(|_| gettext("Missing `decodebin3`\ncheck your gst-plugins-base install"))?;
        gst::ElementFactory::make("gtksink", None)
            .map(drop)
            .map_err(|_| {
                let (major, minor, _micro, _nano) = gst::version();
                let (variant1, variant2) = if major >= 1 && minor >= 14 {
                    ("gstreamer1-plugins-base", "gstreamer1.0-plugins-base")
                } else {
                    (
                        "gstreamer1-plugins-bad-free-gtk",
                        "gstreamer1.0-plugins-bad",
                    )
                };
                format!(
                    "{} {}\n{}",
                    gettext("Couldn't find GStreamer GTK video sink."),
                    gettext("Video playback will be disabled."),
                    gettext("Please install {} or {}, depending on your distribution.")
                        .replacen("{}", variant1, 1)
                        .replacen("{}", variant2, 1),
                )
            })
    }

    fn build_pipeline(&mut self, path: &Path, video_sink: &Option<gst::Element>) {
        let file_src = gst::ElementFactory::make("filesrc", None).unwrap();
        file_src
            .set_property("location", &path.to_str().unwrap())
            .unwrap();

        let decodebin = gst::ElementFactory::make("decodebin3", Some("decodebin")).unwrap();

        let elements = &[&file_src, &decodebin];
        self.pipeline.add_many(elements).unwrap();

        file_src.link(&decodebin).unwrap();

        let audio_sink =
            gst::ElementFactory::make("autoaudiosink", Some("audio_playback_sink")).unwrap();

        // Prepare pad configuration callback
        let pipeline_clone = self.pipeline.clone();
        let video_sink = video_sink.clone();
        decodebin.connect_pad_added(move |_decodebin, src_pad| {
            let pipeline = &pipeline_clone;
            let name = src_pad.get_name();

            if name.starts_with("audio_") {
                let convert = gst::ElementFactory::make("audioconvert", None).unwrap();
                let resample = gst::ElementFactory::make("audioresample", None).unwrap();

                let elements = &[&convert, &resample, &audio_sink];

                pipeline.add_many(elements).unwrap();
                gst::Element::link_many(elements).unwrap();

                for e in elements {
                    e.sync_state_with_parent().unwrap();
                }

                let sink_pad = convert.get_static_pad("sink").unwrap();
                src_pad.link(&sink_pad).unwrap();
            } else if name.starts_with("video_") {
                if let Some(video_sink) = &video_sink {
                    let convert = gst::ElementFactory::make("videoconvert", None).unwrap();
                    let scale = gst::ElementFactory::make("videoscale", None).unwrap();

                    let elements = &[&convert, &scale, video_sink];
                    pipeline.add_many(elements).unwrap();
                    gst::Element::link_many(elements).unwrap();

                    for e in elements {
                        e.sync_state_with_parent().unwrap();
                    }

                    let sink_pad = convert.get_static_pad("sink").unwrap();
                    src_pad.link(&sink_pad).unwrap();
                }
            }
        });
    }

    async fn open(&mut self) -> Result<(), OpenError> {
        let mut bus_stream = self.pipeline.get_bus().unwrap().stream();

        self.pipeline
            .set_state(gst::State::Paused)
            .map(|_| ())
            .map_err(|_| OpenError::StateChange)?;

        let mut missing_plugins = HashSet::<String>::new();
        let mut streams_selected = false;

        while let Some(msg) = bus_stream.next().await {
            use gst::MessageView::*;

            match msg.view() {
                Error(err) => {
                    if "sink" == err.get_src().unwrap().get_name() {
                        // Failure detected on a sink, this occurs when the GL sink
                        // can't operate properly
                        return Err(OpenError::GLSinkError);
                    }

                    return Err(OpenError::Generic(err.get_error().to_string()));
                }
                Element(element_msg) => {
                    let structure = element_msg.get_structure().unwrap();
                    if structure.get_name() == "missing-plugin" {
                        let plugin = structure
                            .get_value("name")
                            .unwrap()
                            .get::<String>()
                            .unwrap()
                            .unwrap();
                        error!(
                            "{}",
                            gettext("Missing plugin: {}").replacen("{}", &plugin, 1)
                        );
                        missing_plugins.insert(plugin);
                    }
                }
                StreamCollection(stream_collection) => {
                    stream_collection
                        .get_stream_collection()
                        .iter()
                        .for_each(|stream| self.info.add_stream(&stream));
                }
                StreamsSelected(_) => streams_selected = true,
                Tag(msg_tag) => {
                    let tags = msg_tag.get_tags();
                    if tags.get_scope() == gst::TagScope::Global {
                        self.info.add_tags(&tags);
                    }
                }
                Toc(msg_toc) => {
                    // FIXME: use updated
                    if self.info.toc.is_none() {
                        let (toc, _updated) = msg_toc.get_toc();
                        if toc.get_scope() == gst::TocScope::Global {
                            self.info.toc = Some(toc);
                        } else {
                            warn!("skipping toc with scope: {:?}", toc.get_scope());
                        }
                    }
                }
                AsyncDone(_) => {
                    if streams_selected {
                        let duration = Duration::from_nanos(
                            self.pipeline
                                .query_duration::<gst::ClockTime>()
                                .unwrap_or_else(|| 0.into())
                                .nanoseconds()
                                .unwrap(),
                        );
                        self.info.duration = duration;

                        break;
                    }
                }
                _ => (),
            }
        }

        if !missing_plugins.is_empty() {
            return Err(OpenError::MissingPlugins(missing_plugins));
        }

        Ok(())
    }

    fn register_bus_watch(
        &mut self,
        ext_msg_tx: async_chan::UnboundedSender<MediaMessage>,
        int_msg_tx: async_chan::UnboundedSender<gst::Message>,
    ) {
        let bus_watch_src_id = self
            .pipeline
            .get_bus()
            .unwrap()
            .add_watch(move |_, msg| {
                use gst::MessageView::*;

                let mut must_forward = false;
                match msg.view() {
                    AsyncDone(_) => must_forward = true,
                    Eos(_) => {
                        ext_msg_tx.unbounded_send(MediaMessage::Eos).unwrap();
                    }
                    Error(err) => {
                        ext_msg_tx
                            .unbounded_send(MediaMessage::Error(err.get_error().to_string()))
                            .unwrap();

                        must_forward = true;
                    }
                    _ => (),
                }

                if must_forward {
                    int_msg_tx.unbounded_send(msg.clone()).unwrap();
                }

                glib::Continue(true)
            })
            .unwrap();

        self.bus_watch_src_id = Some(bus_watch_src_id);
    }
}

/// Operations
impl PlaybackPipeline {
    pub fn current_ts(&self) -> Option<Timestamp> {
        let mut position_query = gst::query::Position::new(gst::Format::Time);
        self.pipeline.query(&mut position_query);
        let position = position_query.get_result().get_value();
        if position < 0 {
            None
        } else {
            Some(position.into())
        }
    }

    /// Purges previous internal messages if any.
    fn purge_int_msg(&mut self) -> Result<(), PurgeError> {
        while let Ok(msg) = self.int_msg_rx.try_next() {
            match msg {
                Some(msg) => {
                    if let gst::MessageView::Error(_) = msg.view() {
                        return Err(PurgeError);
                    }
                }
                None => panic!("internal channel terminated"),
            }
        }

        Ok(())
    }

    pub async fn pause(&mut self) -> Result<(), StateChangeError> {
        self.purge_int_msg()?;

        self.pipeline.set_state(gst::State::Paused)?;

        while let Some(msg) = self.int_msg_rx.next().await {
            use gst::MessageView::*;
            match msg.view() {
                AsyncDone(_) => break,
                Error(_) => return Err(StateChangeError),
                _ => (),
            }
        }

        Ok(())
    }

    pub fn play(&mut self) -> Result<(), StateChangeError> {
        self.purge_int_msg()?;

        self.pipeline.set_state(gst::State::Playing)?;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), StateChangeError> {
        if let Some(bus_watch_src_id) = self.bus_watch_src_id.take() {
            glib::source_remove(bus_watch_src_id);
        }

        self.pipeline.set_state(gst::State::Null)?;
        Ok(())
    }

    pub async fn seek(
        &mut self,
        target: Timestamp,
        flags: gst::SeekFlags,
    ) -> Result<(), SeekError> {
        self.purge_int_msg()?;

        self.pipeline
            .seek_simple(
                gst::SeekFlags::FLUSH | flags,
                ClockTime::from(target.as_u64()),
            )
            .unwrap();

        if target >= self.info.duration {
            return Err(SeekError::Eos);
        }

        use gst::MessageView::*;
        while let Some(msg) = self.int_msg_rx.next().await {
            match msg.view() {
                AsyncDone(_) => break,
                Error(_) => return Err(SeekError::Unrecoverable),
                _ => (),
            }
        }

        Ok(())
    }

    pub async fn select_streams(
        &mut self,
        stream_ids: &[Arc<str>],
    ) -> Result<(), SelectStreamsError> {
        self.purge_int_msg()?;

        let stream_id_vec: Vec<&str> = stream_ids.iter().map(AsRef::as_ref).collect();
        let select_streams_evt = gst::event::SelectStreams::new(&stream_id_vec);
        self.pipeline.send_event(select_streams_evt);

        self.info.streams.select_streams(stream_ids)?;

        Ok(())
    }
}
