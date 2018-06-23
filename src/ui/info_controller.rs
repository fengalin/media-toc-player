use cairo;
use gettextrs::gettext;
use gio;
use gio::prelude::*;
use gtk;
use gtk::prelude::*;
use glib;

use std::fs::File;
use std::rc::{Rc, Weak};
use std::cell::RefCell;

use application::CONFIG;
use media::PlaybackContext;
use metadata;
use metadata::{MediaInfo, Timestamp};

use super::{ChapterTreeManager, ControllerState, ImageSurface, MainController};

const GO_TO_PREV_CHAPTER_THRESHOLD: u64 = 1_000_000_000; // 1 s
const SEEK_STEP: u64 = 1_000_000_000; // 1 s

lazy_static! {
    static ref EMPTY_REPLACEMENT: String = "-".to_owned();
}

pub struct InfoController {
    info_container: gtk::Grid,
    show_chapters_btn: gtk::ToggleButton,

    drawingarea: gtk::DrawingArea,

    title_lbl: gtk::Label,
    artist_lbl: gtk::Label,
    container_lbl: gtk::Label,
    audio_codec_lbl: gtk::Label,
    video_codec_lbl: gtk::Label,
    position_lbl: gtk::Label,
    duration_lbl: gtk::Label,

    timeline_scale: gtk::Scale,
    repeat_btn: gtk::ToggleToolButton,

    chapter_treeview: gtk::TreeView,

    chapter_manager: ChapterTreeManager,

    thumbnail: Option<cairo::ImageSurface>,

    duration: u64,
    repeat_chapter: bool,

    main_ctrl: Option<Weak<RefCell<MainController>>>,
}

impl InfoController {
    pub fn new(builder: &gtk::Builder) -> Rc<RefCell<Self>> {
        let chapter_manager =
            ChapterTreeManager::new_from(builder.get_object("chapters-tree-store").unwrap());
        let chapter_treeview: gtk::TreeView = builder.get_object("chapter-treeview").unwrap();
        chapter_manager.init_treeview(&chapter_treeview);

        // need a RefCell because the callbacks will use immutable versions of ac
        // when the UI controllers will get a mutable version from time to time
        let this_rc = Rc::new(RefCell::new(InfoController {
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

            thumbnail: None,

            chapter_manager,

            duration: 0,
            repeat_chapter: false,

            main_ctrl: None,
        }));

        {
            let mut this = this_rc.borrow_mut();
            this.cleanup();
        }

        this_rc
    }

    pub fn register_callbacks(
        this_rc: &Rc<RefCell<Self>>,
        gtk_app: &gtk::Application,
        main_ctrl: &Rc<RefCell<MainController>>,
    ) {
        let mut this = this_rc.borrow_mut();

        this.main_ctrl = Some(Rc::downgrade(main_ctrl));

        // Show chapters toggle
        if CONFIG.read().unwrap().ui.is_chapters_list_hidden {
            this.show_chapters_btn.set_active(false);
            this.info_container.hide();
        }

        // Register Toggle show chapters list action
        let toggle_show_list = gio::SimpleAction::new("toggle_show_list", None);
        gtk_app.add_action(&toggle_show_list);
        let show_chapters_btn = this.show_chapters_btn.clone();
        toggle_show_list.connect_activate(move |_, _| {
            show_chapters_btn.set_active(!show_chapters_btn.get_active());
        });
        gtk_app.set_accels_for_action("app.toggle_show_list", &["l"]);

        let this_clone = Rc::clone(this_rc);
        this.show_chapters_btn
            .connect_toggled(move |toggle_button| {
                if toggle_button.get_active() {
                    CONFIG.write().unwrap().ui.is_chapters_list_hidden = false;
                    this_clone.borrow().info_container.show();
                } else {
                    CONFIG.write().unwrap().ui.is_chapters_list_hidden = true;
                    this_clone.borrow().info_container.hide();
                }
            });
        this.show_chapters_btn.set_sensitive(true);

        // Draw thumnail image
        let this_clone = Rc::clone(this_rc);
        this.drawingarea
            .connect_draw(move |drawingarea, cairo_ctx| {
                let this = this_clone.borrow();
                this.draw_thumbnail(drawingarea, cairo_ctx)
            });

        // Scale seek
        let main_ctrl_clone = Rc::clone(main_ctrl);
        this.timeline_scale
            .connect_change_value(move |_, _, value| {
                main_ctrl_clone.borrow_mut().seek(value as u64, false); // approximate (fast)
                Inhibit(false)
            });

        // TreeView seek
        let this_clone = Rc::clone(this_rc);
        let main_ctrl_clone = Rc::clone(main_ctrl);
        this.chapter_treeview
            .connect_row_activated(move |_, tree_path, _| {
                let position_opt = {
                    // get the position first in order to make sure
                    // this is no longer borrowed if main_ctrl::seek is to be called
                    let mut this = this_clone.borrow_mut();
                    match this.chapter_manager.get_iter(tree_path) {
                        Some(iter) => {
                            let position = this.chapter_manager.get_chapter_at_iter(&iter).start();
                            // update position
                            this.tick(position, false);
                            Some(position)
                        }
                        None => None,
                    }
                };

                if let Some(position) = position_opt {
                    main_ctrl_clone.borrow_mut().seek(position, true); // accurate (slow)
                }
            });

        // Register Toggle repeat current chapter action
        let toggle_repeat_chapter = gio::SimpleAction::new("toggle_repeat_chapter", None);
        gtk_app.add_action(&toggle_repeat_chapter);
        let repeat_btn = this.repeat_btn.clone();
        toggle_repeat_chapter.connect_activate(move |_, _| {
            repeat_btn.set_active(!repeat_btn.get_active());
        });
        gtk_app.set_accels_for_action("app.toggle_repeat_chapter", &["r"]);

        let this_clone = Rc::clone(this_rc);
        this.repeat_btn.connect_clicked(move |button| {
            this_clone.borrow_mut().repeat_chapter = button.get_active();
        });

        // Register next chapter action
        let next_chapter = gio::SimpleAction::new("next_chapter", None);
        gtk_app.add_action(&next_chapter);
        let this_clone = Rc::clone(this_rc);
        let main_ctrl_clone = Rc::clone(main_ctrl);
        next_chapter.connect_activate(move |_, _| {
            let seek_pos = {
                let this = this_clone.borrow();
                this.chapter_manager
                    .next_iter()
                    .map(|next_iter| this.chapter_manager.get_chapter_at_iter(&next_iter).start())
            };

            if let Some(seek_pos) = seek_pos {
                main_ctrl_clone.borrow_mut().seek(seek_pos, true); // accurate (slow)
            }
        });
        gtk_app.set_accels_for_action("app.next_chapter", &["Down", "AudioNext"]);

        // Register previous chapter action
        let previous_chapter = gio::SimpleAction::new("previous_chapter", None);
        gtk_app.add_action(&previous_chapter);
        let this_clone = Rc::clone(this_rc);
        let main_ctrl_clone = Rc::clone(main_ctrl);
        previous_chapter.connect_activate(move |_, _| {
            let seek_pos = {
                let this = this_clone.borrow();
                let position = this.get_position();
                let cur_start = this.chapter_manager
                    .get_selected_iter()
                    .map(|cur_iter| this.chapter_manager.get_chapter_at_iter(&cur_iter).start());
                let prev_start = this.chapter_manager
                    .prev_iter()
                    .map(|prev_iter| this.chapter_manager.get_chapter_at_iter(&prev_iter).start());

                match (cur_start, prev_start) {
                    (Some(cur_start), prev_start_opt) => {
                        if cur_start + GO_TO_PREV_CHAPTER_THRESHOLD < position {
                            Some(cur_start)
                        } else {
                            prev_start_opt
                        }
                    }
                    (None, prev_start_opt) => prev_start_opt,
                }
            }.unwrap_or(0);

            main_ctrl_clone.borrow_mut().seek(seek_pos, true); // accurate (slow)
        });
        gtk_app.set_accels_for_action("app.previous_chapter", &["Up", "AudioPrev"]);

        // Register Step forward action
        let step_forward = gio::SimpleAction::new("step_forward", None);
        gtk_app.add_action(&step_forward);
        let main_ctrl_clone = Rc::clone(main_ctrl);
        step_forward.connect_activate(move |_, _| {
            let mut main_ctrl = main_ctrl_clone.borrow_mut();
            let seek_pos = {
                main_ctrl.get_position() + SEEK_STEP
            };
            main_ctrl.seek(seek_pos, true); // accurate (slow)
        });
        gtk_app.set_accels_for_action("app.step_forward", &["Right"]);

        // Register Step back action
        let step_back = gio::SimpleAction::new("step_back", None);
        gtk_app.add_action(&step_back);
        let main_ctrl_clone = Rc::clone(main_ctrl);
        step_back.connect_activate(move |_, _| {
            let mut main_ctrl = main_ctrl_clone.borrow_mut();
            let seek_pos = {
                let position = main_ctrl.get_position();
                if position > SEEK_STEP {
                    position - SEEK_STEP
                } else {
                    0
                }
            };
            main_ctrl.seek(seek_pos, true); // accurate (slow)
        });
        gtk_app.set_accels_for_action("app.step_back", &["Left"]);
    }

    fn draw_thumbnail(
        &self,
        drawingarea: &gtk::DrawingArea,
        cairo_ctx: &cairo::Context,
    ) -> Inhibit {
        // Thumbnail draw
        if let Some(ref surface) = self.thumbnail {
            let allocation = drawingarea.get_allocation();
            let alloc_ratio = f64::from(allocation.width) / f64::from(allocation.height);
            let surface_ratio = f64::from(surface.get_width()) / f64::from(surface.get_height());
            let scale = if surface_ratio < alloc_ratio {
                f64::from(allocation.height) / f64::from(surface.get_height())
            } else {
                f64::from(allocation.width) / f64::from(surface.get_width())
            };
            let x =
                (f64::from(allocation.width) / scale - f64::from(surface.get_width())).abs() / 2f64;
            let y = (f64::from(allocation.height) / scale - f64::from(surface.get_height())).abs()
                / 2f64;

            cairo_ctx.scale(scale, scale);
            cairo_ctx.set_source_surface(surface, x, y);
            cairo_ctx.paint();
        }

        Inhibit(true)
    }

    fn show_message(&self, message_type: gtk::MessageType, message: String) {
        let main_ctrl_weak = Weak::clone(self.main_ctrl.as_ref().unwrap());
        gtk::idle_add(move || {
            let main_ctrl_rc = main_ctrl_weak.upgrade().unwrap();
            main_ctrl_rc
                .borrow()
                .show_message(message_type, &message);
            glib::Continue(false)
        });
    }

    fn show_error(&self, message: String) {
        self.show_message(gtk::MessageType::Error, message);
    }

    fn show_info(&self, message: String) {
        self.show_message(gtk::MessageType::Info, message);
    }

    pub fn new_media(&mut self, context: &PlaybackContext) {
        let media_path = context.path.clone();
        let file_stem = media_path.file_stem().unwrap().to_str().unwrap();

        // check the presence of toc files
        let toc_extensions = metadata::Factory::get_extensions();
        let test_path = media_path.clone();
        let mut toc_candidates = toc_extensions
            .into_iter()
            .filter_map(|(extension, format)| {
                let path = test_path
                    .clone()
                    .with_file_name(&format!("{}.{}", file_stem, extension));
                if path.is_file() {
                    Some((path, format))
                } else {
                    None
                }
            });

        {
            let info = context.info.read().unwrap();

            self.duration = info.duration;
            self.timeline_scale.set_range(0f64, info.duration as f64);
            self.duration_lbl
                .set_label(&Timestamp::format(info.duration, false));

            if info.streams.video_selected.is_none() {
                if let Some(ref image_sample) = info.get_image(0) {
                    if let Some(ref image_buffer) = image_sample.get_buffer() {
                        if let Some(ref image_map) = image_buffer.map_readable() {
                            self.thumbnail =
                                ImageSurface::create_from_uknown(image_map.as_slice()).ok();
                        }
                    }
                }

                // show the drawingarea for audio files even when
                // there is no thumbnail so that we get an area
                // with the default background, not the black background
                // of the video widget
                self.drawingarea.show();
                self.drawingarea.queue_draw();
            } else {
                self.drawingarea.hide();
            }

            self.title_lbl
                .set_label(info.get_title().unwrap_or(&EMPTY_REPLACEMENT));
            self.artist_lbl
                .set_label(info.get_artist().unwrap_or(&EMPTY_REPLACEMENT));
            self.container_lbl
                .set_label(info.get_container().unwrap_or(&EMPTY_REPLACEMENT));

            self.streams_changed(&info);

            let extern_toc = toc_candidates
                .next()
                .and_then(|(toc_path, format)| match File::open(toc_path.clone()) {
                    Ok(mut toc_file) => {
                        match metadata::Factory::get_reader(&format).read(&info, &mut toc_file) {
                            Ok(Some(toc)) => Some(toc),
                            Ok(None) => {
                                let msg = gettext("No toc in file \"{}\"")
                                    .replacen(
                                        "{}",
                                        toc_path.file_name().unwrap().to_str().unwrap(),
                                        1,
                                    );
                                info!("{}", msg);
                                self.show_info(msg);
                                None
                            }
                            Err(err) => {
                                self.show_error(
                                    gettext("Error opening toc file \"{}\":\n{}")
                                        .replacen(
                                            "{}",
                                            toc_path.file_name().unwrap().to_str().unwrap(),
                                            1,
                                        )
                                        .replacen("{}", &err, 1)
                                );
                                None
                            }
                        }
                    }
                    Err(_) => {
                        self.show_error(gettext("Failed to open toc file."));
                        None
                    }
                });

            if extern_toc.is_some() {
                self.chapter_manager.replace_with(&extern_toc);
            } else {
                self.chapter_manager.replace_with(&info.toc);
            }
        }

        self.update_marks();

        self.repeat_btn.set_sensitive(true);
        if let Some(current_iter) = self.chapter_manager.get_selected_iter() {
            // position is in a chapter => select it
            self.chapter_treeview
                .get_selection()
                .select_iter(&current_iter);
        }
    }

    pub fn streams_changed(&self, info: &MediaInfo) {
        self.audio_codec_lbl
            .set_label(info.get_audio_codec().unwrap_or(&EMPTY_REPLACEMENT));
        self.video_codec_lbl
            .set_label(info.get_video_codec().unwrap_or(&EMPTY_REPLACEMENT));
    }

    fn update_marks(&self) {
        self.timeline_scale.clear_marks();

        let timeline_scale = self.timeline_scale.clone();
        self.chapter_manager.for_each(None, move |chapter| {
            timeline_scale.add_mark(chapter.start() as f64, gtk::PositionType::Top, None);
            true // keep going until the last chapter
        });
    }

    pub fn cleanup(&mut self) {
        self.title_lbl.set_text("");
        self.artist_lbl.set_text("");
        self.container_lbl.set_text("");
        self.audio_codec_lbl.set_text("");
        self.video_codec_lbl.set_text("");
        self.position_lbl.set_text("00:00.000");
        self.duration_lbl.set_text("00:00.000");
        self.thumbnail = None;
        self.chapter_manager.clear();
        self.timeline_scale.clear_marks();
        self.timeline_scale.set_value(0f64);
        self.duration = 0;
    }

    fn repeat_at(main_ctrl: &Option<Weak<RefCell<MainController>>>, position: u64) {
        let main_ctrl_weak = Weak::clone(main_ctrl.as_ref().unwrap());
        gtk::idle_add(move || {
            let main_ctrl_rc = main_ctrl_weak.upgrade().unwrap();
            main_ctrl_rc.borrow_mut().seek(position, true); // accurate (slow)
            glib::Continue(false)
        });
    }

    pub fn tick(&mut self, position: u64, is_eos: bool) {
        self.timeline_scale.set_value(position as f64);
        self.position_lbl
            .set_text(&Timestamp::format(position, false));

        let (mut has_changed, prev_selected_iter) = self.chapter_manager.update_position(position);

        if self.repeat_chapter {
            // repeat is activated
            if is_eos {
                // postpone chapter selection change until media as synchronized
                has_changed = false;
                self.chapter_manager.rewind();
                InfoController::repeat_at(&self.main_ctrl, 0);
            } else if has_changed {
                if let Some(ref prev_selected_iter) = prev_selected_iter {
                    // discard has_changed because we will be looping on current chapter
                    has_changed = false;

                    // unselect chapter in order to avoid tracing change to current position
                    self.chapter_manager.unselect();
                    InfoController::repeat_at(
                        &self.main_ctrl,
                        self.chapter_manager
                            .get_chapter_at_iter(prev_selected_iter)
                            .start(),
                    );
                }
            }
        }

        if has_changed {
            // chapter has changed
            match self.chapter_manager.get_selected_iter() {
                Some(current_iter) => {
                    // position is in a chapter => select it
                    self.chapter_treeview.get_selection().select_iter(&current_iter);
                }
                None =>
                    // position is not in any chapter
                    if let Some(ref prev_selected_iter) = prev_selected_iter {
                        // but a previous chapter was selected => unselect it
                        self.chapter_treeview.get_selection().unselect_iter(prev_selected_iter);
                    },
            }
        }
    }

    pub fn seek(&mut self, position: u64, state: &ControllerState) {
        self.chapter_manager.prepare_for_seek();

        if *state == ControllerState::Paused {
            // force sync
            self.tick(position, false);
        }
    }

    fn get_position(&self) -> u64 {
        let main_ctrl_rc = self.main_ctrl.as_ref().unwrap().upgrade().unwrap();
        let mut main_ctrl = main_ctrl_rc.borrow_mut();
        main_ctrl.get_position()
    }
}
