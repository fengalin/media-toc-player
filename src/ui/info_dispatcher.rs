use gio;
use gio::prelude::*;
use gstreamer as gst;
use gtk;
use gtk::prelude::*;

use std::{cell::RefCell, rc::Rc};

use crate::application::CONFIG;

use crate::with_main_ctrl;

use super::{ControllerState, MainController, UIDispatcher};

const SEEK_STEP: u64 = 2_500_000_000;

pub struct InfoDispatcher;
impl UIDispatcher for InfoDispatcher {
    fn setup(gtk_app: &gtk::Application, main_ctrl_rc: &Rc<RefCell<MainController>>) {
        let mut main_ctrl = main_ctrl_rc.borrow_mut();
        let info_ctrl = &mut main_ctrl.info_ctrl;

        // Register Toggle show chapters list action
        let toggle_show_list = gio::SimpleAction::new("toggle_show_list", None);
        gtk_app.add_action(&toggle_show_list);
        let show_chapters_btn = info_ctrl.show_chapters_btn.clone();
        toggle_show_list.connect_activate(move |_, _| {
            show_chapters_btn.set_active(!show_chapters_btn.get_active());
        });
        gtk_app.set_accels_for_action("app.toggle_show_list", &["l"]);

        info_ctrl.show_chapters_btn.connect_toggled(with_main_ctrl!(
            main_ctrl_rc => move |&main_ctrl, toggle_button| {
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
        info_ctrl.drawingarea.connect_draw(with_main_ctrl!(
            main_ctrl_rc => move |&mut main_ctrl, drawingarea, cairo_ctx| {
                main_ctrl.info_ctrl.draw_thumbnail(drawingarea, cairo_ctx);
                Inhibit(true)
            }
        ));

        // Scale seek
        info_ctrl
            .timeline_scale
            .connect_change_value(with_main_ctrl!(
                main_ctrl_rc => move |&mut main_ctrl, _, _, value| {
                    main_ctrl.seek(value as u64, gst::SeekFlags::KEY_UNIT);
                    Inhibit(true)
                }
            ));

        // TreeView seek
        info_ctrl
            .chapter_treeview
            .connect_row_activated(with_main_ctrl!(
                main_ctrl_rc => move |&mut main_ctrl, _, tree_path, _| {
                    let info_ctrl = &mut main_ctrl.info_ctrl;
                    if let Some(iter) = info_ctrl.chapter_manager.get_iter(tree_path) {
                        let position = info_ctrl.chapter_manager.get_chapter_at_iter(&iter).start();
                        // update position
                        info_ctrl.tick(position, ControllerState::Seeking);
                        main_ctrl.seek(position, gst::SeekFlags::ACCURATE);
                    }
                }
            ));

        // Register Toggle repeat current chapter action
        let toggle_repeat_chapter = gio::SimpleAction::new("toggle_repeat_chapter", None);
        gtk_app.add_action(&toggle_repeat_chapter);
        let repeat_btn = info_ctrl.repeat_btn.clone();
        toggle_repeat_chapter.connect_activate(move |_, _| {
            repeat_btn.set_active(!repeat_btn.get_active());
        });
        gtk_app.set_accels_for_action("app.toggle_repeat_chapter", &["r"]);

        info_ctrl.repeat_btn.connect_clicked(with_main_ctrl!(
            main_ctrl_rc => move |&mut main_ctrl, button| {
                main_ctrl.info_ctrl.repeat_chapter = button.get_active();
            }
        ));

        // Register next chapter action
        let next_chapter = gio::SimpleAction::new("next_chapter", None);
        gtk_app.add_action(&next_chapter);
        next_chapter.connect_activate(with_main_ctrl!(
            main_ctrl_rc => move |&mut main_ctrl, _, _| {
                let seek_pos = main_ctrl
                    .info_ctrl
                    .chapter_manager
                    .next_iter()
                    .map(|next_iter| {
                        main_ctrl
                            .info_ctrl
                            .chapter_manager
                            .get_chapter_at_iter(&next_iter)
                            .start()
                    });

                if let Some(seek_pos) = seek_pos {
                    main_ctrl.seek(seek_pos, gst::SeekFlags::ACCURATE);
                }
            }
        ));
        gtk_app.set_accels_for_action("app.next_chapter", &["Down", "AudioNext"]);

        // Register previous chapter action
        let previous_chapter = gio::SimpleAction::new("previous_chapter", None);
        gtk_app.add_action(&previous_chapter);
        previous_chapter.connect_activate(with_main_ctrl!(
            main_ctrl_rc => move |&mut main_ctrl, _, _| {
                let position = main_ctrl.get_position();
                let seek_pos = main_ctrl.info_ctrl.previous_pos(position);
                main_ctrl.seek(seek_pos, gst::SeekFlags::ACCURATE);
            }
        ));
        gtk_app.set_accels_for_action("app.previous_chapter", &["Up", "AudioPrev"]);

        // Register Step forward action
        let step_forward = gio::SimpleAction::new("step_forward", None);
        gtk_app.add_action(&step_forward);
        step_forward.connect_activate(with_main_ctrl!(
            main_ctrl_rc => move |&mut main_ctrl, _, _| {
                let seek_target = main_ctrl.get_position() + SEEK_STEP;
                main_ctrl.seek(seek_target, gst::SeekFlags::ACCURATE);
            }
        ));
        gtk_app.set_accels_for_action("app.step_forward", &["Right"]);

        // Register Step back action
        let step_back = gio::SimpleAction::new("step_back", None);
        gtk_app.add_action(&step_back);
        step_back.connect_activate(with_main_ctrl!(
            main_ctrl_rc => move |&mut main_ctrl, _, _| {
                let seek_pos = {
                    let ts = main_ctrl.get_position();
                    if ts > SEEK_STEP {
                        ts - SEEK_STEP
                    } else {
                        0
                    }
                };
                main_ctrl.seek(seek_pos, gst::SeekFlags::ACCURATE);
            }
        ));
        gtk_app.set_accels_for_action("app.step_back", &["Left"]);
    }
}
