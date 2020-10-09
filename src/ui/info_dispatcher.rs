use gio::prelude::*;
use glib::clone;
use gstreamer as gst;
use gtk::prelude::*;

use std::{cell::RefCell, rc::Rc};

use crate::{application::CONFIG, media::Timestamp, metadata::Duration};

use super::{InfoController, MainController, UIDispatcher, UIEventSender, UIFocusContext};

const GO_TO_PREV_CHAPTER_THRESHOLD: Duration = Duration::from_secs(1);
const SEEK_STEP: Duration = Duration::from_nanos(2_500_000_000);

pub struct InfoDispatcher;
impl UIDispatcher for InfoDispatcher {
    type Controller = InfoController;

    fn setup(
        info_ctrl: &mut InfoController,
        main_ctrl_rc: &Rc<RefCell<MainController>>,
        app: &gtk::Application,
        ui_event: &UIEventSender,
    ) {
        // Register Toggle show chapters list action
        let toggle_show_list = gio::SimpleAction::new("toggle_show_list", None);
        app.add_action(&toggle_show_list);
        let show_chapters_btn = info_ctrl.show_chapters_btn.clone();
        toggle_show_list.connect_activate(move |_, _| {
            show_chapters_btn.set_active(!show_chapters_btn.get_active());
        });

        info_ctrl.show_chapters_btn.connect_toggled(clone!(
            @weak main_ctrl_rc => move |toggle_button| {
                let main_ctrl = main_ctrl_rc.borrow();
                let info_ctrl = &main_ctrl.info_ctrl;
                if toggle_button.get_active() {
                    CONFIG.write().unwrap().ui.is_chapters_list_hidden = false;
                    info_ctrl.info_container.show();
                } else {
                    CONFIG.write().unwrap().ui.is_chapters_list_hidden = true;
                    info_ctrl.info_container.hide();
                }
            }
        ));

        // Draw thumnail image
        info_ctrl.drawingarea.connect_draw(clone!(
            @weak main_ctrl_rc => @default-return Inhibit(true), move |drawingarea, cairo_ctx| {
                let mut main_ctrl = main_ctrl_rc.borrow_mut();
                main_ctrl.info_ctrl.draw_thumbnail(drawingarea, cairo_ctx);
                Inhibit(true)
            }
        ));

        // Scale seek
        info_ctrl.timeline_scale.connect_change_value(
            clone!(@strong ui_event => move |_, _, value| {
                ui_event.seek((value as u64).into(), gst::SeekFlags::KEY_UNIT);
                Inhibit(true)
            }),
        );

        // TreeView seek
        info_ctrl.chapter_treeview.connect_row_activated(
            clone!(@weak main_ctrl_rc, @strong ui_event => move |_, tree_path, _| {
                let seek_ts = {
                    let mut main_ctrl = main_ctrl_rc.borrow_mut();
                    let info_ctrl = &mut main_ctrl.info_ctrl;
                    info_ctrl.chapter_manager.chapter_from_path(tree_path).map(|chapter| {
                        chapter.start()
                    })
                };

                if let Some(seek_ts) = seek_ts {
                    ui_event.seek(seek_ts, gst::SeekFlags::ACCURATE);
                }
            }),
        );

        // Register Toggle repeat current chapter action
        let toggle_repeat_chapter = gio::SimpleAction::new("toggle_repeat_chapter", None);
        app.add_action(&toggle_repeat_chapter);
        let repeat_btn = info_ctrl.repeat_btn.clone();
        toggle_repeat_chapter.connect_activate(move |_, _| {
            repeat_btn.set_active(!repeat_btn.get_active());
        });

        info_ctrl
            .repeat_btn
            .connect_clicked(clone!(@weak main_ctrl_rc => move |button| {
                let mut main_ctrl = main_ctrl_rc.borrow_mut();
                main_ctrl.info_ctrl.repeat_chapter = button.get_active();
            }));

        // Register next chapter action
        app.add_action(&info_ctrl.next_chapter_action);
        info_ctrl.next_chapter_action.connect_activate(
            clone!(@weak main_ctrl_rc, @strong ui_event => move |_, _| {
                let seek_pos = main_ctrl_rc.borrow()
                    .info_ctrl
                    .chapter_manager
                    .pick_next()
                    .map(|next_chapter| next_chapter.start());

                if let Some(seek_pos) = seek_pos {
                    ui_event.seek(seek_pos, gst::SeekFlags::ACCURATE);
                }
            }),
        );

        // Register previous chapter action
        app.add_action(&info_ctrl.previous_chapter_action);
        info_ctrl.previous_chapter_action.connect_activate(clone!(
            @weak main_ctrl_rc, @strong ui_event => move |_, _| {
                let seek_ts = {
                    let mut main_ctrl = main_ctrl_rc.borrow_mut();
                    let cur_ts = main_ctrl.current_ts();
                    cur_ts.and_then(|cur_ts| {
                        let cur_start = main_ctrl
                            .info_ctrl
                            .chapter_manager
                            .selected()
                            .map(|sel_chapter| sel_chapter.start());
                        let prev_start = main_ctrl
                            .info_ctrl
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
                    })
                };

                ui_event.seek(seek_ts.unwrap_or_else(Timestamp::default), gst::SeekFlags::ACCURATE);
            }
        ));

        // Register Step forward action
        let step_forward = gio::SimpleAction::new("step_forward", None);
        app.add_action(&step_forward);
        step_forward.connect_activate(clone!(
            @weak main_ctrl_rc, @strong ui_event => move |_, _| {
                let current_ts = main_ctrl_rc.borrow_mut().current_ts();
                if let Some(current_ts) = current_ts {
                    let seek_target = current_ts + SEEK_STEP;
                    ui_event.seek(seek_target, gst::SeekFlags::ACCURATE);
                }
            }
        ));
        app.set_accels_for_action("app.step_forward", &["Right"]);

        // Register Step back action
        let step_back = gio::SimpleAction::new("step_back", None);
        app.add_action(&step_back);
        step_back.connect_activate(clone!(
            @weak main_ctrl_rc, @strong ui_event => move |_, _| {
                let current_ts = main_ctrl_rc.borrow_mut().current_ts();
                if let Some(current_ts) = current_ts {
                    let seek_pos = current_ts.saturating_sub(SEEK_STEP);
                    ui_event.seek(seek_pos, gst::SeekFlags::ACCURATE);
                }
            }
        ));

        app.set_accels_for_action("app.step_back", &["Left"]);
    }

    fn bind_accels_for(ctx: UIFocusContext, app: &gtk::Application) {
        match ctx {
            UIFocusContext::PlaybackPage => {
                app.set_accels_for_action("app.toggle_show_list", &["l"]);
                app.set_accels_for_action("app.toggle_repeat_chapter", &["r"]);
            }
            UIFocusContext::StreamsPage => {
                app.set_accels_for_action("app.toggle_show_list", &["l"]);
                app.set_accels_for_action("app.toggle_repeat_chapter", &["r"]);
            }
            UIFocusContext::InfoBar => {
                app.set_accels_for_action("app.toggle_show_list", &[]);
                app.set_accels_for_action("app.toggle_repeat_chapter", &[]);
            }
        }
    }
}
