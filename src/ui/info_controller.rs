use cairo;

use gtk;
use gtk::prelude::*;

use glib;

use std::fs::File;

use std::rc::{Rc, Weak};
use std::cell::RefCell;

use media::Context;

use metadata;
use metadata::{MediaInfo, Timestamp};

use super::{ChapterTreeManager, ControllerState, ImageSurface, MainController};

lazy_static! {
    static ref EMPTY_REPLACEMENT: String = "-".to_owned();
}

pub struct InfoController {
    info_container: gtk::Grid,

    drawingarea: gtk::DrawingArea,

    title_lbl: gtk::Label,
    artist_lbl: gtk::Label,
    container_lbl: gtk::Label,
    audio_codec_lbl: gtk::Label,
    video_codec_lbl: gtk::Label,
    position_lbl: gtk::Label,
    duration_lbl: gtk::Label,

    timeline_scale: gtk::Scale,
    repeat_button: gtk::ToggleToolButton,
    show_chapters_button: gtk::ToggleButton,

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

            drawingarea: builder.get_object("thumbnail-drawingarea").unwrap(),

            title_lbl: builder.get_object("title-lbl").unwrap(),
            artist_lbl: builder.get_object("artist-lbl").unwrap(),
            container_lbl: builder.get_object("container-lbl").unwrap(),
            audio_codec_lbl: builder.get_object("audio_codec-lbl").unwrap(),
            video_codec_lbl: builder.get_object("video_codec-lbl").unwrap(),
            position_lbl: builder.get_object("position-lbl").unwrap(),
            duration_lbl: builder.get_object("duration-lbl").unwrap(),

            timeline_scale: builder.get_object("timeline-scale").unwrap(),
            repeat_button: builder.get_object("repeat-toolbutton").unwrap(),
            show_chapters_button: builder.get_object("show_chapters-toggle").unwrap(),

            chapter_treeview: chapter_treeview,

            thumbnail: None,

            chapter_manager: chapter_manager,

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
        main_ctrl: &Rc<RefCell<MainController>>,
    ) {
        let mut this = this_rc.borrow_mut();

        this.main_ctrl = Some(Rc::downgrade(main_ctrl));

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

        // repeat button
        let this_clone = Rc::clone(this_rc);
        this.repeat_button.connect_clicked(move |button| {
            this_clone.borrow_mut().repeat_chapter = button.get_active();
        });

        let this_clone = Rc::clone(this_rc);
        this.show_chapters_button
            .connect_toggled(move |toggle_button| {
                if toggle_button.get_active() {
                    this_clone.borrow().info_container.show();
                } else {
                    this_clone.borrow().info_container.hide();
                }
            });
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

    pub fn new_media(&mut self, context: &Context) {
        let media_path = context.path.clone();
        let file_stem = media_path
            .file_stem()
            .expect("InfoController::new_media clicked, failed to get file_stem")
            .to_str()
            .expect("InfoController::new_media clicked, failed to get file_stem as str");

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
            let info = context
                .info
                .lock()
                .expect("InfoController::new_media failed to lock media info");

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

            match toc_candidates.next() {
                Some((toc_path, format)) => {
                    let mut toc_file = File::open(toc_path)
                        .expect("InfoController::new_media failed to open toc file");
                    self.chapter_manager.replace_with(&metadata::Factory::get_reader(&format)
                        .read(&info, &mut toc_file)
                    );
                }
                None => self.chapter_manager.replace_with(&info.toc),
            }

            self.update_marks();

            if let Some(current_iter) = self.chapter_manager.get_selected_iter() {
                // position is in a chapter => select it
                self.chapter_treeview.get_selection().select_iter(&current_iter);
            }
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
            let main_ctrl_rc = main_ctrl_weak
                .upgrade()
                .expect("InfoController::tick can't upgrade main_ctrl while repeating chapter");
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
}
