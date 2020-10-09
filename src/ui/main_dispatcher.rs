use futures::prelude::*;

use gettextrs::gettext;

use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;

use std::{cell::RefCell, rc::Rc, time::Duration};

use super::{
    InfoDispatcher, MainController, PerspectiveDispatcher, PlaybackPipeline, StreamsDispatcher,
    UIDispatcher, UIFocusContext, VideoDispatcher,
};

const TRACKER_PERIOD: u64 = 40; //  40 ms (25 Hz)

pub struct MainDispatcher;
impl MainDispatcher {
    pub fn setup(
        main_ctrl: &mut MainController,
        main_ctrl_rc: &Rc<RefCell<MainController>>,
        app: &gtk::Application,
    ) {
        let app_menu = gio::Menu::new();
        app.set_app_menu(Some(&app_menu));

        let app_section = gio::Menu::new();
        app_menu.append_section(None, &app_section);

        // About
        let about = gio::SimpleAction::new("about", None);
        app.add_action(&about);
        about.connect_activate(clone!(@weak main_ctrl_rc => move |_, _| {
            main_ctrl_rc.borrow().about();
        }));
        app.set_accels_for_action("app.about", &["<Ctrl>A"]);
        app_section.append(Some(&gettext("About")), Some("app.about"));

        // Quit
        let quit = gio::SimpleAction::new("quit", None);
        app.add_action(&quit);
        quit.connect_activate(clone!(@weak main_ctrl_rc => move |_, _| {
            main_ctrl_rc.borrow_mut().quit();
        }));
        app.set_accels_for_action("app.quit", &["<Ctrl>Q"]);
        app_section.append(Some(&gettext("Quit")), Some("app.quit"));

        main_ctrl.window.connect_delete_event(
            clone!(@weak main_ctrl_rc => @default-panic, move |_, _| {
                main_ctrl_rc.borrow_mut().quit();
                Inhibit(false)
            }),
        );

        let ui_event = main_ctrl.ui_event().clone();
        if gstreamer::init().is_ok() {
            PerspectiveDispatcher::setup(
                &mut main_ctrl.perspective_ctrl,
                main_ctrl_rc,
                &app,
                &ui_event,
            );
            VideoDispatcher::setup(&mut main_ctrl.video_ctrl, main_ctrl_rc, &app, &ui_event);
            InfoDispatcher::setup(&mut main_ctrl.info_ctrl, main_ctrl_rc, &app, &ui_event);
            StreamsDispatcher::setup(&mut main_ctrl.streams_ctrl, main_ctrl_rc, &app, &ui_event);

            main_ctrl.new_tracker = Some(Box::new(clone!(@weak main_ctrl_rc =>
            @default-panic, move || {
                let main_ctrl_rc = Rc::clone(&main_ctrl_rc);
                async move {
                    loop {
                        glib::timeout_future(Duration::from_millis(TRACKER_PERIOD)).await;
                        if let Ok(mut main_ctrl) = main_ctrl_rc.try_borrow_mut() {
                            main_ctrl.tick();
                        }
                    }
                }.boxed_local()
            })));

            let _ = PlaybackPipeline::check_requirements()
                .map_err(clone!(@strong ui_event => move |err| ui_event.show_error(err)));

            let main_section = gio::Menu::new();
            app_menu.insert_section(0, None, &main_section);

            // Register Open action
            let open = gio::SimpleAction::new("open", None);
            app.add_action(&open);
            open.connect_activate(clone!(@strong ui_event => move |_, _| ui_event.select_media()));
            main_section.append(Some(&gettext("Open media file")), Some("app.open"));
            app.set_accels_for_action("app.open", &["<Ctrl>O"]);

            main_ctrl.open_btn.set_sensitive(true);

            // Register Play/Pause action
            let play_pause = gio::SimpleAction::new("play_pause", None);
            app.add_action(&play_pause);
            play_pause.connect_activate(clone!(@strong ui_event => move |_, _| {
                ui_event.play_pause();
            }));
            main_ctrl.play_pause_btn.set_sensitive(true);

            main_ctrl
                .display_page
                .connect_map(clone!(@strong ui_event => move |_| {
                    ui_event.switch_to(UIFocusContext::PlaybackPage);
                }));

            ui_event.switch_to(UIFocusContext::PlaybackPage);
        } else {
            // GStreamer initialization failed
            let msg = gettext("Failed to initialize GStreamer, the application can't be used.");
            ui_event.show_error(msg);
        }
    }
}
