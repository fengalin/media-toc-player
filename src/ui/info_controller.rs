use gettextrs::gettext;
use gtk::prelude::*;
use log::{debug, info, warn};

use std::fs::File;

use crate::{
    application::CONFIG,
    media::{PlaybackPipeline, Timestamp},
    metadata,
    metadata::{Duration, MediaInfo, Timestamp4Humans},
};

use super::{
    ChapterTreeManager, ControllerState, Image, PositionStatus, UIController, UIEventSender,
};

const EMPTY_REPLACEMENT: &str = "-";
const GO_TO_PREV_CHAPTER_THRESHOLD: Duration = Duration::from_secs(1);
pub const SEEK_STEP: Duration = Duration::from_nanos(2_500_000_000);

enum ThumbnailState {
    Blocked,
    Unblocked,
}

struct Thumbnail {
    drawingarea: gtk::DrawingArea,
    signal_handler_id: Option<glib::SignalHandlerId>,
    state: ThumbnailState,
}

impl Thumbnail {
    fn new<D>(drawingarea: &gtk::DrawingArea, draw_cb: D) -> Self
    where
        D: Fn(&gtk::DrawingArea, &cairo::Context) -> Inhibit + 'static,
    {
        let signal_handler_id = drawingarea.connect_draw(draw_cb);
        glib::signal_handler_block(drawingarea, &signal_handler_id);

        Thumbnail {
            drawingarea: drawingarea.clone(),
            signal_handler_id: Some(signal_handler_id),
            state: ThumbnailState::Blocked,
        }
    }

    fn block(&mut self) {
        if let ThumbnailState::Unblocked = self.state {
            glib::signal_handler_block(&self.drawingarea, self.signal_handler_id.as_ref().unwrap());
            self.state = ThumbnailState::Blocked;
        }
    }

    fn unblock(&mut self) {
        if let ThumbnailState::Blocked = self.state {
            glib::signal_handler_unblock(
                &self.drawingarea,
                self.signal_handler_id.as_ref().unwrap(),
            );
            self.state = ThumbnailState::Unblocked;
        }
    }
}

impl Drop for Thumbnail {
    fn drop(&mut self) {
        glib::signal_handler_disconnect(&self.drawingarea, self.signal_handler_id.take().unwrap());
    }
}

pub struct InfoController {
    ui_event: UIEventSender,

    pub(super) info_container: gtk::Grid,
    pub(super) show_chapters_btn: gtk::ToggleButton,

    pub(super) drawingarea: gtk::DrawingArea,

    title_lbl: gtk::Label,
    artist_lbl: gtk::Label,
    container_lbl: gtk::Label,
    audio_codec_lbl: gtk::Label,
    video_codec_lbl: gtk::Label,
    position_lbl: gtk::Label,
    duration_lbl: gtk::Label,

    pub(super) timeline_scale: gtk::Scale,
    pub(super) repeat_btn: gtk::ToggleToolButton,

    pub(super) chapter_treeview: gtk::TreeView,
    pub(super) next_chapter_action: gio::SimpleAction,
    pub(super) previous_chapter_action: gio::SimpleAction,

    thumbnail: Option<Thumbnail>,

    pub(super) chapter_manager: ChapterTreeManager,

    duration: Duration,
    pub(super) repeat_chapter: bool,
}

impl UIController for InfoController {
    fn new_media(&mut self, pipeline: &PlaybackPipeline) {
        let toc_extensions = metadata::Factory::get_extensions();

        {
            // check the presence of a toc file
            let mut toc_candidates =
                toc_extensions
                    .into_iter()
                    .filter_map(|(extension, format)| {
                        let path = pipeline
                            .info
                            .path
                            .with_file_name(&format!("{}.{}", pipeline.info.name, extension));
                        if path.is_file() {
                            Some((path, format))
                        } else {
                            None
                        }
                    });

            self.duration = pipeline.info.duration;
            self.timeline_scale
                .set_range(0f64, pipeline.info.duration.as_f64());
            self.duration_lbl
                .set_label(&Timestamp4Humans::from_duration(pipeline.info.duration).to_string());

            let thumbnail = pipeline.info.media_image().and_then(|image| {
                image.get_buffer().and_then(|image_buffer| {
                    image_buffer.map_readable().ok().and_then(|image_map| {
                        Image::from_unknown(image_map.as_slice())
                            .map_err(|err| warn!("{}", err))
                            .ok()
                    })
                })
            });

            if let Some(thumbnail) = thumbnail {
                self.thumbnail = Some(Thumbnail::new(
                    &self.drawingarea,
                    move |drawingarea, cairo_ctx| {
                        Self::draw_thumbnail(&thumbnail, drawingarea, cairo_ctx);
                        Inhibit(true)
                    },
                ));
            }

            self.container_lbl
                .set_label(pipeline.info.container().unwrap_or(EMPTY_REPLACEMENT));

            let extern_toc = toc_candidates
                .next()
                .and_then(|(toc_path, format)| match File::open(toc_path.clone()) {
                    Ok(mut toc_file) => {
                        match metadata::Factory::get_reader(format)
                            .read(&pipeline.info, &mut toc_file)
                        {
                            Ok(Some(toc)) => Some(toc),
                            Ok(None) => {
                                let msg = gettext("No toc in file \"{}\"").replacen(
                                    "{}",
                                    toc_path.file_name().unwrap().to_str().unwrap(),
                                    1,
                                );
                                info!("{}", msg);
                                self.ui_event.show_info(msg);
                                None
                            }
                            Err(err) => {
                                self.ui_event.show_error(
                                    gettext("Error opening toc file \"{}\":\n{}")
                                        .replacen(
                                            "{}",
                                            toc_path.file_name().unwrap().to_str().unwrap(),
                                            1,
                                        )
                                        .replacen("{}", &err, 1),
                                );
                                None
                            }
                        }
                    }
                    Err(_) => {
                        self.ui_event
                            .show_error(gettext("Failed to open toc file."));
                        None
                    }
                });

            if extern_toc.is_some() {
                self.chapter_manager.replace_with(&extern_toc);
            } else {
                self.chapter_manager.replace_with(&pipeline.info.toc);
            }
        }

        self.update_marks();

        self.repeat_btn.set_sensitive(true);
        if let Some(sel_chapter) = self.chapter_manager.selected() {
            // position is in a chapter => select it
            self.chapter_treeview
                .get_selection()
                .select_iter(sel_chapter.iter());
        }

        self.next_chapter_action.set_enabled(true);
        self.previous_chapter_action.set_enabled(true);

        self.ui_event.update_focus();
    }

    fn cleanup(&mut self) {
        self.title_lbl.set_text("");
        self.artist_lbl.set_text("");
        self.container_lbl.set_text("");
        self.audio_codec_lbl.set_text("");
        self.video_codec_lbl.set_text("");
        self.position_lbl.set_text("00:00.000");
        self.duration_lbl.set_text("00:00.000");
        let _ = self.thumbnail.take();
        self.chapter_treeview.get_selection().unselect_all();
        self.chapter_manager.clear();
        self.next_chapter_action.set_enabled(false);
        self.previous_chapter_action.set_enabled(false);
        self.timeline_scale.clear_marks();
        self.timeline_scale.set_value(0f64);
        self.duration = Duration::default();
    }

    fn streams_changed(&mut self, info: &MediaInfo) {
        match info.media_artist() {
            Some(artist) => self.artist_lbl.set_label(artist),
            None => self.artist_lbl.set_label(EMPTY_REPLACEMENT),
        }
        match info.media_title() {
            Some(title) => self.title_lbl.set_label(title),
            None => self.title_lbl.set_label(EMPTY_REPLACEMENT),
        }

        self.audio_codec_lbl
            .set_label(info.streams.audio_codec().unwrap_or(EMPTY_REPLACEMENT));
        self.video_codec_lbl
            .set_label(info.streams.video_codec().unwrap_or(EMPTY_REPLACEMENT));

        if !info.streams.is_video_selected() {
            debug!("streams_changed showing thumbnail");
            if let Some(thumbnail) = self.thumbnail.as_mut() {
                thumbnail.unblock();
            }
            self.drawingarea.show();
            self.drawingarea.queue_draw();
        } else {
            if let Some(thumbnail) = self.thumbnail.as_mut() {
                thumbnail.block();
            }
            self.drawingarea.hide();
        }
    }

    fn grab_focus(&self) {
        self.chapter_treeview.grab_focus();

        match self.chapter_manager.selected_path() {
            Some(sel_path) => {
                self.chapter_treeview
                    .set_cursor(&sel_path, None::<&gtk::TreeViewColumn>, false);
                self.chapter_treeview.grab_default();
            }
            None => {
                // Set the cursor to an uninitialized path to unselect
                self.chapter_treeview.set_cursor(
                    &gtk::TreePath::new(),
                    None::<&gtk::TreeViewColumn>,
                    false,
                );
            }
        }
    }
}

impl InfoController {
    pub fn new(builder: &gtk::Builder, ui_event: UIEventSender) -> Self {
        let mut chapter_manager =
            ChapterTreeManager::new(builder.get_object("chapters-tree-store").unwrap());
        let chapter_treeview: gtk::TreeView = builder.get_object("chapter-treeview").unwrap();
        chapter_manager.init_treeview(&chapter_treeview);

        let mut ctrl = InfoController {
            ui_event,

            info_container: builder.get_object("info-chapter_list-grid").unwrap(),
            show_chapters_btn: builder.get_object("show_chapters-toggle").unwrap(),

            drawingarea: builder.get_object("thumbnail-drawingarea").unwrap(),

            title_lbl: builder.get_object("title-lbl").unwrap(),
            artist_lbl: builder.get_object("artist-lbl").unwrap(),
            container_lbl: builder.get_object("container-lbl").unwrap(),
            audio_codec_lbl: builder.get_object("audio_codec-lbl").unwrap(),
            video_codec_lbl: builder.get_object("video_codec-lbl").unwrap(),
            position_lbl: builder.get_object("position-lbl").unwrap(),
            duration_lbl: builder.get_object("duration-lbl").unwrap(),

            timeline_scale: builder.get_object("timeline-scale").unwrap(),
            repeat_btn: builder.get_object("repeat-toolbutton").unwrap(),

            chapter_treeview,
            next_chapter_action: gio::SimpleAction::new("next_chapter", None),
            previous_chapter_action: gio::SimpleAction::new("previous_chapter", None),

            thumbnail: None,

            chapter_manager,

            duration: Duration::default(),
            repeat_chapter: false,
        };

        ctrl.cleanup();

        // Show chapters toggle
        if CONFIG.read().unwrap().ui.is_chapters_list_hidden {
            ctrl.show_chapters_btn.set_active(false);
            ctrl.info_container.hide();
        }

        ctrl.show_chapters_btn.set_sensitive(true);

        ctrl
    }

    pub fn draw_thumbnail(
        image: &Image,
        drawingarea: &gtk::DrawingArea,
        cairo_ctx: &cairo::Context,
    ) {
        let allocation = drawingarea.get_allocation();
        let alloc_width_f: f64 = allocation.width.into();
        let alloc_height_f: f64 = allocation.height.into();

        let image_width_f: f64 = image.width().into();
        let image_height_f: f64 = image.height().into();

        let alloc_ratio = alloc_width_f / alloc_height_f;
        let image_ratio = image_width_f / image_height_f;
        let scale = if image_ratio < alloc_ratio {
            alloc_height_f / image_height_f
        } else {
            alloc_width_f / image_width_f
        };
        let x = (alloc_width_f / scale - image_width_f).abs() / 2f64;
        let y = (alloc_height_f / scale - image_height_f).abs() / 2f64;

        image.with_surface_external_context(cairo_ctx, |cr, surface| {
            cr.scale(scale, scale);
            cr.set_source_surface(surface, x, y);
            cr.paint();
        })
    }

    fn update_marks(&self) {
        self.timeline_scale.clear_marks();

        let timeline_scale = self.timeline_scale.clone();
        self.chapter_manager.iter().for_each(move |chapter| {
            timeline_scale.add_mark(chapter.start().as_f64(), gtk::PositionType::Top, None);
        });
    }

    fn repeat_at(&self, ts: Timestamp) {
        self.ui_event.seek(ts, gst::SeekFlags::ACCURATE)
    }

    pub fn tick(&mut self, ts: Timestamp, state: ControllerState) {
        self.timeline_scale.set_value(ts.as_f64());
        self.position_lbl
            .set_text(&Timestamp4Humans::from_nano(ts.as_u64()).to_string());

        let mut position_status = self.chapter_manager.update_ts(ts);

        if self.repeat_chapter {
            // repeat is activated
            if let ControllerState::EosPlaying = state {
                // postpone chapter selection change until media has synchronized
                position_status = PositionStatus::ChapterNotChanged;
                self.repeat_at(Timestamp::default());
            } else if let PositionStatus::ChapterChanged { prev_chapter } = &position_status {
                if let Some(prev_chapter) = prev_chapter {
                    // reset position_status because we will be looping on current chapter
                    let prev_start = prev_chapter.start;
                    position_status = PositionStatus::ChapterNotChanged;

                    // unselect chapter in order to avoid tracing change to current timestamp
                    self.chapter_manager.unselect();
                    self.repeat_at(prev_start);
                }
            }
        }

        if let PositionStatus::ChapterChanged { prev_chapter } = position_status {
            // let go the mutable reference on `self.chapter_manager`
            match self.chapter_manager.selected() {
                Some(sel_chapter) => {
                    // timestamp is in a chapter => select it
                    self.chapter_treeview
                        .get_selection()
                        .select_iter(sel_chapter.iter());
                }
                None =>
                // timestamp is not in any chapter
                {
                    if let Some(prev_chapter) = prev_chapter {
                        // but a previous chapter was selected => unselect it
                        self.chapter_treeview
                            .get_selection()
                            .unselect_iter(&prev_chapter.iter);
                    }
                }
            }

            self.ui_event.update_focus();
        }
    }

    pub fn seek(&mut self, target: Timestamp, state: ControllerState) {
        self.tick(target, state);
    }

    pub fn toggle_chapter_list(&self, must_show: bool) {
        CONFIG.write().unwrap().ui.is_chapters_list_hidden = must_show;

        if must_show {
            self.info_container.hide();
        } else {
            self.info_container.show();
        }
    }

    pub fn previous_chapter(&self, cur_ts: Timestamp) -> Option<Timestamp> {
        let cur_start = self
            .chapter_manager
            .selected()
            .map(|sel_chapter| sel_chapter.start());
        let prev_start = self
            .chapter_manager
            .pick_previous()
            .map(|prev_chapter| prev_chapter.start());

        match (cur_start, prev_start) {
            (Some(cur_start), prev_start_opt) => {
                if cur_ts > cur_start + GO_TO_PREV_CHAPTER_THRESHOLD {
                    Some(cur_start)
                } else {
                    prev_start_opt
                }
            }
            (None, prev_start_opt) => prev_start_opt,
        }
    }
}
