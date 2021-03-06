use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;

use std::{cell::RefCell, rc::Rc};

use super::{InfoController, MainController, UIDispatcher, UIEventSender, UIFocusContext};

pub struct InfoDispatcher;
impl UIDispatcher for InfoDispatcher {
    type Controller = InfoController;

    fn setup(
        info_ctrl: &mut InfoController,
        _main_ctrl_rc: &Rc<RefCell<MainController>>,
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
            @strong ui_event => move |toggle_button| {
                ui_event.toggle_chapter_list(!toggle_button.get_active());
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
            clone!(@strong ui_event => move |_, tree_path, _| {
                ui_event.chapter_clicked(tree_path.clone());
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
            .connect_clicked(clone!(@strong ui_event => move |button| {
                ui_event.toggle_repeat(button.get_active());
            }));

        // Register next chapter action
        app.add_action(&info_ctrl.next_chapter_action);
        info_ctrl
            .next_chapter_action
            .connect_activate(clone!(@strong ui_event => move |_, _| {
                ui_event.next_chapter();
            }));

        // Register previous chapter action
        app.add_action(&info_ctrl.previous_chapter_action);
        info_ctrl.previous_chapter_action.connect_activate(clone!(
            @strong ui_event => move |_, _| {
                ui_event.previous_chapter();
            }
        ));

        // Register Step forward action
        let step_forward = gio::SimpleAction::new("step_forward", None);
        app.add_action(&step_forward);
        step_forward.connect_activate(clone!(
            @strong ui_event => move |_, _| {
                ui_event.step_forward();
            }
        ));
        app.set_accels_for_action("app.step_forward", &["Right"]);

        // Register Step back action
        let step_back = gio::SimpleAction::new("step_back", None);
        app.add_action(&step_back);
        step_back.connect_activate(clone!(
            @strong ui_event => move |_, _| {
                ui_event.step_back();
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
