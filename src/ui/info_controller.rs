extern crate cairo;

extern crate gtk;
use gtk::prelude::*;

extern crate glib;

use std::rc::{Rc, Weak};
use std::cell::RefCell;

use media::{Context, Timestamp};

use super::{ImageSurface, MainController};

const START_COL: u32 = 1;
const END_COL: u32 = 2;
const TITLE_COL: u32 = 3;
const START_STR_COL: u32 = 4;
const END_STR_COL: u32 = 5;

pub struct InfoController {
    drawingarea: gtk::DrawingArea,

    title_lbl: gtk::Label,
    artist_lbl: gtk::Label,
    container_lbl: gtk::Label,
    audio_codec_lbl: gtk::Label,
    video_codec_lbl: gtk::Label,
    duration_lbl: gtk::Label,

    timeline_scale: gtk::Scale,
    repeat_button: gtk::ToggleToolButton,

    chapter_treeview: gtk::TreeView,
    chapter_store: gtk::TreeStore,

    thumbnail: Option<ImageSurface>,

    duration: u64,
    chapter_iter: Option<gtk::TreeIter>,
    repeat_chapter: bool,

    main_ctrl: Option<Weak<RefCell<MainController>>>,
}

impl InfoController {
    pub fn new(builder: &gtk::Builder) -> Rc<RefCell<Self>> {
        // need a RefCell because the callbacks will use immutable versions of ac
        // when the UI controllers will get a mutable version from time to time
        let this_rc = Rc::new(RefCell::new(InfoController {
            drawingarea: builder.get_object("thumbnail-drawingarea").unwrap(),

            title_lbl: builder.get_object("title-lbl").unwrap(),
            artist_lbl: builder.get_object("artist-lbl").unwrap(),
            container_lbl: builder.get_object("container-lbl").unwrap(),
            audio_codec_lbl: builder.get_object("audio_codec-lbl").unwrap(),
            video_codec_lbl: builder.get_object("video_codec-lbl").unwrap(),
            duration_lbl: builder.get_object("duration-lbl").unwrap(),

            timeline_scale: builder.get_object("timeline-scale").unwrap(),
            repeat_button: builder.get_object("repeat-toolbutton").unwrap(),

            chapter_treeview: builder.get_object("chapter-treeview").unwrap(),
            chapter_store: builder.get_object("chapters-tree-store").unwrap(),

            thumbnail: None,

            duration: 0,
            chapter_iter: None,
            repeat_chapter: false,

            main_ctrl: None,
        }));

        {
            let mut this = this_rc.borrow_mut();
            this.cleanup();

            this.chapter_treeview.set_model(Some(&this.chapter_store));
            this.add_chapter_column("Title", TITLE_COL as i32, true);
            this.add_chapter_column("Start", START_STR_COL as i32, false);
            this.add_chapter_column("End", END_STR_COL as i32, false);

            let this_clone = Rc::clone(&this_rc);
            this.drawingarea.connect_draw(move |drawingarea, cairo_ctx| {
                this_clone.borrow()
                    .draw_thumbnail(drawingarea, cairo_ctx)
                    .into()
            });

            let this_clone = Rc::clone(&this_rc);
            this.repeat_button.connect_clicked(move |button| {
                this_clone.borrow_mut().repeat_chapter =
                    button.get_active();
            });
        }

        this_rc
    }

    fn add_chapter_column(&self, title: &str, col_id: i32, can_expand: bool) {
        let col = gtk::TreeViewColumn::new();
        col.set_title(title);
        let renderer = gtk::CellRendererText::new();
        col.pack_start(&renderer, true);
        col.add_attribute(&renderer, "text", col_id);
        if can_expand {
            col.set_min_width(70);
            col.set_expand(can_expand);
        }
        self.chapter_treeview.append_column(&col);
    }

    pub fn register_callbacks(&mut self, main_ctrl: &Rc<RefCell<MainController>>) {
        self.main_ctrl = Some(Rc::downgrade(&main_ctrl));

        // Scale seek
        let main_ctrl_rc = Rc::clone(main_ctrl);
        self.timeline_scale.connect_change_value(move |_, _, value| {
            main_ctrl_rc.borrow_mut().seek(value as u64, false); // approximate (fast)
            Inhibit(true)
        });

        // TreeView seek
        let chapter_store = self.chapter_store.clone();
        let main_ctrl_rc = Rc::clone(main_ctrl);
        self.chapter_treeview.set_activate_on_single_click(true);
        self.chapter_treeview.connect_row_activated(move |_, tree_path, _| {
            if let Some(chapter_iter) = chapter_store.get_iter(tree_path) {
                let position = chapter_store.get_value(&chapter_iter, START_COL as i32)
                                    .get::<u64>().unwrap();
                main_ctrl_rc.borrow_mut().seek(position, true); // accurate (slow)
            }
        });
    }

    fn draw_thumbnail(&self,
        drawingarea: &gtk::DrawingArea,
        cairo_ctx: &cairo::Context,
    ) -> Inhibit {
        // Thumbnail draw
        if let Some(ref thumbnail) = self.thumbnail {
            let surface = &thumbnail.surface;

            let allocation = drawingarea.get_allocation();
            let alloc_ratio = f64::from(allocation.width)
                / f64::from(allocation.height);
            let surface_ratio = f64::from(surface.get_width())
                / f64::from(surface.get_height());
            let scale = if surface_ratio < alloc_ratio {
                f64::from(allocation.height)
                / f64::from(surface.get_height())
            }
            else {
                f64::from(allocation.width)
                / f64::from(surface.get_width())
            };
            let x = (
                    f64::from(allocation.width) / scale - f64::from(surface.get_width())
                ).abs() / 2f64;
            let y = (
                f64::from(allocation.height) / scale - f64::from(surface.get_height())
                ).abs() / 2f64;

            cairo_ctx.scale(scale, scale);
            cairo_ctx.set_source_surface(surface, x, y);
            cairo_ctx.paint();
        }

        Inhibit(true)
    }

    pub fn new_media(&mut self, context: &Context) {
        self.update_duration(context.get_duration());

        self.chapter_store.clear();

        {
            let mut info = context.info.lock()
                .expect("Failed to lock media info in InfoController");

            if let Some(thumbnail) = info.thumbnail.take() {
                if let Ok(image) = ImageSurface::from_aligned_image(thumbnail) {
                    self.thumbnail = Some(image);
                }
            };

            self.title_lbl.set_label(&info.title);
            self.artist_lbl.set_label(&info.artist);
            // Fix container for mp3 audio files TODO: move this to MediaInfo
            let container = if info.video_codec.is_empty()
                && info.audio_codec.to_lowercase().find("mp3").is_some()
            {
                "MP3"
            }
            else {
                &info.container
            };
            self.container_lbl.set_label(container);
            self.audio_codec_lbl.set_label(
                if !info.audio_codec.is_empty() { &info.audio_codec } else { "-" }
            );
            self.video_codec_lbl.set_label(
                if !info.video_codec.is_empty() { &info.video_codec } else { "-" }
            );

            self.chapter_iter = None;

            // FIX for sample.mkv video: generate ids (TODO: remove)
            for chapter in info.chapters.iter() {
                self.chapter_store.insert_with_values(
                    None, None,
                    &[START_COL, END_COL, TITLE_COL, START_STR_COL, END_STR_COL],
                    &[
                        &chapter.start.nano_total,
                        &chapter.end.nano_total,
                        &chapter.title(),
                        &format!("{}", &chapter.start),
                        &format!("{}", chapter.end),
                    ],
                );
            }

            self.update_marks();

            self.chapter_iter = self.chapter_store.get_iter_first();
        }

        if self.thumbnail.is_some() {
            self.drawingarea.show();
            self.drawingarea.queue_draw();
        }
        else {
            self.drawingarea.hide();
        }
    }

    fn update_marks(&self) {
        self.timeline_scale.clear_marks();

        if let Some(chapter_iter) = self.chapter_store.get_iter_first() {
            let mut keep_going = true;
            while keep_going {
                let start =
                    self.chapter_store.get_value(&chapter_iter, START_COL as i32)
                        .get::<u64>().unwrap();
                self.timeline_scale.add_mark(
                    start as f64,
                    gtk::PositionType::Top,
                    None
                );
                keep_going = self.chapter_store.iter_next(&chapter_iter);
            }
        }
    }

    pub fn cleanup(&mut self) {
        self.title_lbl.set_text("");
        self.artist_lbl.set_text("");
        self.container_lbl.set_text("");
        self.audio_codec_lbl.set_text("");
        self.video_codec_lbl.set_text("");
        self.duration_lbl.set_text("00:00.000");
        self.thumbnail = None;
        self.chapter_store.clear();
        self.timeline_scale.clear_marks();
        self.timeline_scale.set_value(0f64);
        self.duration = 0;
        self.chapter_iter = None;
    }

    pub fn update_duration(&mut self, duration: u64) {
        self.duration = duration;
        self.timeline_scale.set_range(0f64, duration as f64);
        self.duration_lbl.set_label(
            &format!("{}", Timestamp::format(duration, false))
        );
    }

    pub fn tick(&mut self, position: u64, is_eos: bool) {
        self.timeline_scale.set_value(position as f64);

        let mut done_with_chapters = false;

        if let Some(current_iter) = self.chapter_iter.as_mut() {
            let current_start =
                self.chapter_store.get_value(current_iter, START_COL as i32)
                    .get::<u64>().unwrap();
            if position < current_start
            {   // before selected chapter
                // (first chapter must start after the beginning of the stream)
                return;
            } else if is_eos
                || position >= self.chapter_store.get_value(current_iter, END_COL as i32)
                    .get::<u64>().unwrap()
            {   // passed the end of current chapter
                if self.repeat_chapter {
                    // seek back to the beginning of the chapter
                    let main_ctrl_weak = Weak::clone(self.main_ctrl.as_ref().unwrap());
                    gtk::idle_add(move || {
                        let main_ctrl_rc = main_ctrl_weak.upgrade()
                            .expect("InfoController::tick can't upgrade main_ctrl while repeating chapter");
                        main_ctrl_rc.borrow_mut().seek(current_start, true); // accurate
                        glib::Continue(false)
                    });
                    return;
                }

                // unselect current chapter
                self.chapter_treeview.get_selection()
                    .unselect_iter(current_iter);

                if !self.chapter_store.iter_next(current_iter) {
                    // no more chapters
                    done_with_chapters = true;
                }
            }

            if !done_with_chapters
            && position >= self.chapter_store.get_value(current_iter, START_COL as i32)
                    .get::<u64>().unwrap() // after current start
            && position < self.chapter_store.get_value(current_iter, END_COL as i32)
                    .get::<u64>().unwrap()
            { // before current end
                self.chapter_treeview.get_selection()
                    .select_iter(current_iter);
            }
        }

        if done_with_chapters {
            self.chapter_iter = None;
        }
    }

    pub fn seek(&mut self, position: u64) {
        self.timeline_scale.set_value(position as f64);

        if let Some(first_iter) = self.chapter_store.get_iter_first() {
            // chapters available => update with new position
            let mut keep_going = true;

            let current_iter =
                if let Some(current_iter) = self.chapter_iter.take() {
                    if position
                        < self.chapter_store.get_value(&current_iter, START_COL as i32)
                            .get::<u64>().unwrap()
                    {   // new position before current chapter's start
                        // unselect current chapter
                        self.chapter_treeview.get_selection()
                            .unselect_iter(&current_iter);

                        // rewind to first chapter
                        first_iter
                    } else if position
                        >= self.chapter_store.get_value(&current_iter, END_COL as i32)
                            .get::<u64>().unwrap()
                    {   // new position after current chapter's end
                        // unselect current chapter
                        self.chapter_treeview.get_selection()
                            .unselect_iter(&current_iter);

                        if !self.chapter_store.iter_next(&current_iter) {
                            // no more chapters
                            keep_going = false;
                        }
                        current_iter
                    } else {
                        // new position still in current chapter
                        self.chapter_iter = Some(current_iter);
                        return;
                    }
                } else {
                    first_iter
                };

            let mut set_chapter_iter = false;
            while keep_going {
                if position
                    < self.chapter_store.get_value(&current_iter, START_COL as i32)
                        .get::<u64>().unwrap()
                {   // new position before selected chapter's start
                    set_chapter_iter = true;
                    keep_going = false;
                } else if position
                    >= self.chapter_store.get_value(&current_iter, START_COL as i32)
                        .get::<u64>().unwrap()
                && position
                    < self.chapter_store.get_value(&current_iter, END_COL as i32)
                        .get::<u64>().unwrap()
                {   // after current start and before current end
                    self.chapter_treeview.get_selection()
                        .select_iter(&current_iter);
                    set_chapter_iter = true;
                    keep_going = false;
                } else {
                    if !self.chapter_store.iter_next(&current_iter) {
                        // no more chapters
                        keep_going = false;
                    }
                }
            }

            if set_chapter_iter {
                self.chapter_iter = Some(current_iter);
            }
        }
    }
}
