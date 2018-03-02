use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer::{BinExt, ClockTime, ElementFactory, GstObjectExt, PadExt};

use glib;
use glib::ObjectExt;

use gtk;

use std::path::PathBuf;

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use metadata::MediaInfo;

use super::ContextMessage;

// The video_sink must be created in the main UI thread
// as it contains a gtk::Widget
// GLGTKSink not used because it causes high CPU usage on some systems.
lazy_static! {
    static ref VIDEO_SINK: gst::Element =
        ElementFactory::make("gtksink", "video_sink")
            .expect(concat!(
                r#"Couldn't find GStreamer GTK video sink. Please install "#,
                r#"gstreamer1-plugins-bad-free-gtk or gstreamer1.0-plugins-bad, "#,
                r#"depenging on your distribution."#
            ));
}

#[derive(Clone, Debug, PartialEq)]
pub enum PipelineState {
    None,
    Initialized,
    StreamsStarted,
    StreamsSelected,
}

pub struct Context {
    pipeline: gst::Pipeline,
    decodebin: gst::Element,
    position_element: Option<gst::Element>,
    position_query: gst::query::Position<gst::Query>,

    pub path: PathBuf,
    pub file_name: String,
    pub name: String,

    pub info: Arc<Mutex<MediaInfo>>,
}

// FIXME: might need to `release_request_pad` on the tee
impl Drop for Context {
    fn drop(&mut self) {
        if let Some(video_sink) = self.pipeline.get_by_name("video_sink") {
            self.pipeline.remove(&video_sink).unwrap();
        }
    }
}

impl Context {
    pub fn new(path: PathBuf, ctx_tx: Sender<ContextMessage>) -> Result<Context, String> {
        println!("\n\n* Opening {:?}...", path);

        let file_name = String::from(path.file_name().unwrap().to_str().unwrap());

        let mut this = Context {
            pipeline: gst::Pipeline::new("pipeline"),
            decodebin: gst::ElementFactory::make("decodebin3", None).unwrap(),
            position_element: None,
            position_query: gst::Query::new_position(gst::Format::Time),

            file_name: file_name.clone(),
            name: String::from(path.file_stem().unwrap().to_str().unwrap()),
            path: path,

            info: Arc::new(Mutex::new(MediaInfo::new())),
        };

        this.pipeline.add(&this.decodebin).unwrap();

        this.info
            .lock()
            .expect("Context::new failed to lock media info")
            .file_name = file_name;

        this.build_pipeline((*VIDEO_SINK).clone());
        this.register_bus_inspector(ctx_tx);

        match this.play() {
            Ok(_) => Ok(this),
            Err(error) => Err(error),
        }
    }

    pub fn get_video_widget() -> gtk::Widget {
        let widget_val = (*VIDEO_SINK).get_property("widget").unwrap();
        widget_val
            .get::<gtk::Widget>()
            .expect("Failed to get GstGtkWidget glib::Value as gtk::Widget")
    }

    pub fn get_position(&mut self) -> u64 {
        let pipeline = self.pipeline.clone();
        self.position_element
            .get_or_insert_with(|| {
                if let Some(video) = pipeline.get_by_name("video_sink") {
                    video
                } else if let Some(audio) = pipeline.get_by_name("audio_playback_sink") {
                    audio
                } else {
                    panic!("No sink in pipeline");
                }
            })
            .query(&mut self.position_query);
        self.position_query.get_result().get_value() as u64
    }

    pub fn get_state(&self) -> gst::State {
        let (_, current, _) = self.pipeline.get_state(ClockTime::from(10_000_000));
        current
    }

    pub fn play(&self) -> Result<(), String> {
        if self.pipeline.set_state(gst::State::Playing) == gst::StateChangeReturn::Failure {
            return Err("Could not set media in palying state".into());
        }
        Ok(())
    }

    pub fn pause(&self) -> Result<(), String> {
        if self.pipeline.set_state(gst::State::Paused) == gst::StateChangeReturn::Failure {
            return Err("Could not set media in Paused state".into());
        }
        Ok(())
    }

    pub fn stop(&self) {
        if self.pipeline.set_state(gst::State::Null) == gst::StateChangeReturn::Failure {
            println!("Could not set media in Null state");
            //return Err("could not set media in Null state".into());
        }
    }

    pub fn seek(&self, position: u64, accurate: bool) {
        let flags = gst::SeekFlags::FLUSH | if accurate {
            gst::SeekFlags::ACCURATE
        } else {
            gst::SeekFlags::KEY_UNIT
        };
        self.pipeline
            .seek_simple(flags, ClockTime::from(position))
            .ok()
            .unwrap();
    }

    pub fn select_streams(&self, stream_ids: &[String]) {
        let stream_ids: Vec<&str> = stream_ids.iter().map(|id| id.as_str()).collect();
        let select_streams_evt = gst::Event::new_select_streams(&stream_ids).build();
        self.decodebin.send_event(select_streams_evt);

        {
            let mut info = self.info
                .lock()
                .expect("MainController::select_streams failed to lock info");
            info.streams.select_streams(&stream_ids);
        }
    }

    // TODO: handle errors
    fn build_pipeline(&mut self, video_sink: gst::Element) {
        let file_src = gst::ElementFactory::make("filesrc", None).unwrap();
        file_src
            .set_property("location", &gst::Value::from(self.path.to_str().unwrap()))
            .unwrap();

        self.pipeline.add(&file_src).unwrap();
        file_src.link(&self.decodebin).unwrap();

        let audio_sink = gst::ElementFactory::make("autoaudiosink", "audio_playback_sink").unwrap();

        // Prepare pad configuration callback
        let pipeline_clone = self.pipeline.clone();
        self.decodebin
            .connect_pad_added(move |_decodebin, src_pad| {
                let pipeline = &pipeline_clone;
                let name = src_pad.get_name();

                if name.starts_with("audio_") {
                    let queue = gst::ElementFactory::make("queue", "playback_queue").unwrap();

                    let convert = gst::ElementFactory::make("audioconvert", None).unwrap();
                    let resample = gst::ElementFactory::make("audioresample", None).unwrap();

                    let elements = &[&queue, &convert, &resample, &audio_sink];

                    pipeline.add_many(elements).unwrap();
                    gst::Element::link_many(elements).unwrap();

                    for e in elements {
                        e.sync_state_with_parent().unwrap();
                    }

                    let sink_pad = queue.get_static_pad("sink").unwrap();
                    assert_eq!(src_pad.link(&sink_pad), gst::PadLinkReturn::Ok);
                } else if name.starts_with("video_") {
                    let queue = gst::ElementFactory::make("queue", None).unwrap();
                    let convert = gst::ElementFactory::make("videoconvert", None).unwrap();
                    let scale = gst::ElementFactory::make("videoscale", None).unwrap();

                    let elements = &[&queue, &convert, &scale, &video_sink];
                    pipeline.add_many(elements).unwrap();
                    gst::Element::link_many(elements).unwrap();

                    for e in elements {
                        e.sync_state_with_parent().unwrap();
                    }

                    let sink_pad = queue.get_static_pad("sink").unwrap();
                    assert_eq!(src_pad.link(&sink_pad), gst::PadLinkReturn::Ok);
                }
            });
    }

    // Uses ctx_tx to notify the UI controllers about the inspection process
    fn register_bus_inspector(&self, ctx_tx: Sender<ContextMessage>) {
        let mut pipeline_state = PipelineState::None;
        let info_arc_mtx = Arc::clone(&self.info);
        let pipeline = self.pipeline.clone();
        self.pipeline.get_bus().unwrap().add_watch(move |_, msg| {
            match msg.view() {
                gst::MessageView::Eos(..) => {
                    ctx_tx
                        .send(ContextMessage::Eos)
                        .expect("Failed to notify UI");
                }
                gst::MessageView::Error(err) => {
                    eprintln!(
                        "Error from {}: {} ({:?})",
                        msg.get_src()
                            .map(|s| s.get_path_string(),)
                            .unwrap_or_else(|| String::from("None"),),
                        err.get_error(),
                        err.get_debug()
                    );
                    ctx_tx
                        .send(ContextMessage::FailedToOpenMedia)
                        .expect("Failed to notify UI");
                    return glib::Continue(false);
                }
                gst::MessageView::AsyncDone(_) => {
                    if pipeline_state == PipelineState::StreamsSelected {
                        pipeline_state = PipelineState::Initialized;
                        {
                            let info = &mut info_arc_mtx
                                .lock()
                                .expect("Failed to lock media info while setting duration");
                            info.duration = pipeline
                                .query_duration::<gst::ClockTime>()
                                .unwrap_or_else(|| 0.into())
                                .nanoseconds()
                                .unwrap();
                        }
                        ctx_tx
                            .send(ContextMessage::InitDone)
                            .expect("Failed to notify UI");
                    } else if pipeline_state == PipelineState::Initialized {
                        ctx_tx
                            .send(ContextMessage::AsyncDone)
                            .expect("Failed to notify UI");
                    }
                }
                gst::MessageView::Tag(msg_tag) => {
                    if pipeline_state != PipelineState::Initialized {
                        let info = &mut info_arc_mtx
                            .lock()
                            .expect("Failed to lock media info while reading tags");
                        info.tags = info.tags
                            .merge(&msg_tag.get_tags(), gst::TagMergeMode::Replace);
                    }
                }
                gst::MessageView::Toc(msg_toc) => {
                    if pipeline_state != PipelineState::Initialized {
                        // FIXME: use updated
                        let (toc, _updated) = msg_toc.get_toc();
                        if toc.get_scope() == gst::TocScope::Global {
                            let info = &mut info_arc_mtx
                                .lock()
                                .expect("Failed to lock media info while receiving toc");
                            info.toc = Some(toc);
                        } else {
                            println!("Warning: Skipping toc with scope: {:?}", toc.get_scope());
                        }
                    }
                }
                gst::MessageView::StreamStart(_) => {
                    if pipeline_state == PipelineState::None {
                        pipeline_state = PipelineState::StreamsStarted;
                    }
                }
                gst::MessageView::StreamsSelected(_) => {
                    if pipeline_state == PipelineState::Initialized {
                        ctx_tx
                            .send(ContextMessage::StreamsSelected)
                            .expect("Failed to notify UI");
                    } else {
                        pipeline_state = PipelineState::StreamsSelected;
                    }
                }
                gst::MessageView::StreamCollection(msg_stream_collection) => {
                    let stream_collection = msg_stream_collection.get_stream_collection();
                    let info = &mut info_arc_mtx
                        .lock()
                        .expect("Failed to lock media info while initializing audio stream");
                    stream_collection
                        .iter()
                        .for_each(|stream| info.streams.add_stream(&stream));
                }
                _ => (),
            }

            glib::Continue(true)
        });
    }
}
