use futures::channel::mpsc as async_mpsc;

use gettextrs::gettext;

use gstreamer as gst;

use gstreamer::{prelude::*, ClockTime};

use log::{info, warn};

use std::{
    convert::AsRef,
    path::Path,
    sync::{Arc, RwLock},
};

use crate::metadata::{Duration, MediaInfo};

use super::{MediaEvent, PlaybackState, Timestamp};

#[derive(PartialEq)]
pub enum PipelineState {
    None,
    Playable(PlaybackState),
    StreamsSelected,
}

pub struct PlaybackPipeline {
    pipeline: gst::Pipeline,
    decodebin: gst::Element,

    pub info: Arc<RwLock<MediaInfo>>,
}

// FIXME: might need to `release_request_pad` on the tee
impl Drop for PlaybackPipeline {
    fn drop(&mut self) {
        if let Some(video_sink) = self.pipeline.get_by_name("video_sink") {
            self.pipeline.remove(&video_sink).unwrap();
        }
    }
}

impl PlaybackPipeline {
    pub fn try_new(
        path: &Path,
        video_sink: &Option<gst::Element>,
        sender: async_mpsc::Sender<MediaEvent>,
    ) -> Result<PlaybackPipeline, String> {
        info!(
            "{}",
            gettext("Opening {}...").replacen("{}", path.to_str().unwrap(), 1)
        );

        let mut this = PlaybackPipeline {
            pipeline: gst::Pipeline::new(Some("playback_pipeline")),
            decodebin: gst::ElementFactory::make("decodebin3", Some("decodebin")).unwrap(),

            info: Arc::new(RwLock::new(MediaInfo::new(path))),
        };

        this.pipeline.add(&this.decodebin).unwrap();
        this.build_pipeline(path, video_sink);
        this.register_bus_inspector(sender);

        this.pause().map(|_| this)
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

    pub fn get_current_ts(&self) -> Option<Timestamp> {
        let mut position_query = gst::query::Position::new(gst::Format::Time);
        self.pipeline.query(&mut position_query);
        let position = position_query.get_result().get_value();
        if position < 0 {
            None
        } else {
            Some(position.into())
        }
    }

    pub fn get_state(&self) -> gst::State {
        let (_, current, _) = self.pipeline.get_state(ClockTime::from(10_000_000));
        current
    }

    pub fn play(&mut self) -> Result<(), String> {
        self.pipeline
            .set_state(gst::State::Playing)
            .map(|_| ())
            .map_err(|_| gettext("Could not set media in Playing mode"))
    }

    pub fn pause(&self) -> Result<(), String> {
        self.pipeline
            .set_state(gst::State::Paused)
            .map(|_| ())
            .map_err(|_| gettext("Could not set media in Paused mode"))
    }

    pub fn stop(&self) {
        if self.pipeline.set_state(gst::State::Null).is_err() {
            warn!("could not stop the media");
        }
    }

    pub fn seek(&self, target: Timestamp, flags: gst::SeekFlags) {
        self.pipeline
            .seek_simple(
                gst::SeekFlags::FLUSH | flags,
                ClockTime::from(target.as_u64()),
            )
            .ok()
            .unwrap();
    }

    pub fn select_streams(&self, stream_ids: &[Arc<str>]) {
        let stream_id_vec: Vec<&str> = stream_ids.iter().map(AsRef::as_ref).collect();
        let select_streams_evt = gst::event::SelectStreams::new(&stream_id_vec);
        self.decodebin.send_event(select_streams_evt);

        {
            let mut info = self.info.write().unwrap();
            info.streams.select_streams(stream_ids);
        }
    }

    fn build_pipeline(&mut self, path: &Path, video_sink: &Option<gst::Element>) {
        let file_src = gst::ElementFactory::make("filesrc", None).unwrap();
        file_src
            .set_property("location", &glib::Value::from(path.to_str().unwrap()))
            .unwrap();

        self.pipeline.add(&file_src).unwrap();
        file_src.link(&self.decodebin).unwrap();

        let audio_sink =
            gst::ElementFactory::make("autoaudiosink", Some("audio_playback_sink")).unwrap();

        // Prepare pad configuration callback
        let pipeline_clone = self.pipeline.clone();
        let video_sink = video_sink.clone();
        self.decodebin
            .connect_pad_added(move |_decodebin, src_pad| {
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

    // Uses sender to notify the UI controllers about the inspection process
    fn register_bus_inspector(&self, mut sender: async_mpsc::Sender<MediaEvent>) {
        let mut pipeline_state = PipelineState::None;
        let info_arc_mtx = Arc::clone(&self.info);
        let pipeline = self.pipeline.clone();
        self.pipeline
            .get_bus()
            .unwrap()
            .add_watch(move |_, msg| {
                match msg.view() {
                    gst::MessageView::Eos(_) => {
                        sender
                            .try_send(MediaEvent::Eos)
                            .expect("Failed to notify UI");
                    }
                    gst::MessageView::Error(err) => {
                        if "sink" == err.get_src().unwrap().get_name() {
                            // Failure detected on a sink, this occurs when the GL sink
                            // can't operate properly
                            sender.try_send(MediaEvent::GLSinkError).unwrap();
                        } else {
                            sender
                                .try_send(MediaEvent::FailedToOpenMedia(
                                    err.get_error().to_string(),
                                ))
                                .unwrap();
                        }
                        return glib::Continue(false);
                    }
                    gst::MessageView::Element(element_msg) => {
                        let structure = element_msg.get_structure().unwrap();
                        if structure.get_name() == "missing-plugin" {
                            sender
                                .try_send(MediaEvent::MissingPlugin(
                                    structure
                                        .get_value("name")
                                        .unwrap()
                                        .get::<String>()
                                        .unwrap()
                                        .unwrap(),
                                ))
                                .unwrap();
                        }
                    }
                    gst::MessageView::AsyncDone(_) => match pipeline_state {
                        PipelineState::Playable(playback_state) => {
                            sender
                                .try_send(MediaEvent::AsyncDone(playback_state))
                                .expect("Failed to notify UI");
                        }
                        PipelineState::StreamsSelected => {
                            pipeline_state = PipelineState::Playable(PlaybackState::Paused);
                            let duration = Duration::from_nanos(
                                pipeline
                                    .query_duration::<gst::ClockTime>()
                                    .unwrap_or_else(|| 0.into())
                                    .nanoseconds()
                                    .unwrap(),
                            );
                            info_arc_mtx
                                .write()
                                .expect("Failed to lock media info while setting duration")
                                .duration = duration;

                            sender
                                .try_send(MediaEvent::InitDone)
                                .expect("Failed to notify UI");
                        }
                        _ => (),
                    },
                    gst::MessageView::StateChanged(msg_state_changed) => {
                        if let PipelineState::Playable(_) = pipeline_state {
                            if let Some(source) = msg_state_changed.get_src() {
                                if source.get_type() != gst::Pipeline::static_type() {
                                    return glib::Continue(true);
                                }

                                match msg_state_changed.get_current() {
                                    gst::State::Playing => {
                                        pipeline_state =
                                            PipelineState::Playable(PlaybackState::Playing);
                                    }
                                    gst::State::Paused => {
                                        if msg_state_changed.get_old() != gst::State::Paused {
                                            pipeline_state =
                                                PipelineState::Playable(PlaybackState::Paused);
                                            sender.try_send(MediaEvent::ReadyToRefresh).unwrap();
                                        }
                                    }
                                    _ => unreachable!(format!(
                                        "PlaybackPipeline bus inspector, `StateChanged` to {:?}",
                                        msg_state_changed.get_current(),
                                    )),
                                }
                            }
                        }
                    }
                    gst::MessageView::Tag(msg_tag) => match pipeline_state {
                        PipelineState::Playable(_) => (),
                        _ => {
                            let tags = msg_tag.get_tags();
                            if tags.get_scope() == gst::TagScope::Global {
                                info_arc_mtx
                                    .write()
                                    .expect("Failed to lock media info while receiving tags")
                                    .add_tags(&tags);
                            }
                        }
                    },
                    gst::MessageView::Toc(msg_toc) => {
                        match pipeline_state {
                            PipelineState::Playable(_) => (),
                            _ => {
                                // FIXME: use updated
                                if info_arc_mtx.write().unwrap().toc.is_none() {
                                    let (toc, _updated) = msg_toc.get_toc();
                                    if toc.get_scope() == gst::TocScope::Global {
                                        info_arc_mtx.write().unwrap().toc = Some(toc);
                                    } else {
                                        warn!("skipping toc with scope: {:?}", toc.get_scope());
                                    }
                                }
                            }
                        }
                    }
                    gst::MessageView::StreamCollection(stream_collection) => {
                        let info = &mut info_arc_mtx.write().unwrap();

                        stream_collection
                            .get_stream_collection()
                            .iter()
                            .for_each(|stream| info.add_stream(&stream));
                    }
                    gst::MessageView::StreamsSelected(_) => match pipeline_state {
                        PipelineState::Playable(_) => {
                            sender.try_send(MediaEvent::StreamsSelected).unwrap();
                        }
                        PipelineState::None => {
                            let has_usable_streams = {
                                let info = info_arc_mtx.read().unwrap();
                                info.streams.is_audio_selected() || info.streams.is_video_selected()
                            };

                            if has_usable_streams {
                                pipeline_state = PipelineState::StreamsSelected;
                            } else {
                                sender
                                    .try_send(MediaEvent::FailedToOpenMedia(gettext(
                                        "No usable streams could be found.",
                                    )))
                                    .unwrap();
                                return glib::Continue(false);
                            }
                        }
                        PipelineState::StreamsSelected => unreachable!(concat!(
                            "PlaybackPipeline received msg `StreamsSelected` while already ",
                            "being in state `StreamsSelected`",
                        )),
                    },
                    _ => (),
                }

                glib::Continue(true)
            })
            .unwrap();
    }
}
