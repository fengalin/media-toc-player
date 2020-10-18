use futures::channel::mpsc as async_mpsc;
use futures::prelude::*;

use gdk::{Cursor, CursorType, WindowExt};

use gettextrs::gettext;

use gio::prelude::*;
use glib::clone;
use gtk::prelude::*;

use log::debug;

use std::{cell::RefCell, rc::Rc, time::Duration};

use super::{
    spawn, ui_event::UIEvent, InfoBarController, InfoDispatcher, MainController,
    PerspectiveDispatcher, PlaybackPipeline, StreamsDispatcher, UIController, UIDispatcher,
    UIFocusContext, VideoDispatcher,
};

const TRACKER_PERIOD: u64 = 40; //  40 ms (25 Hz)

pub struct MainDispatcher {
    app: gtk::Application,
    window: gtk::ApplicationWindow,
    main_ctrl: Rc<RefCell<MainController>>,
    info_bar_ctrl: InfoBarController,
    saved_context: Option<UIFocusContext>,
    focus: UIFocusContext,
}

impl MainDispatcher {
    pub(super) fn setup(
        main_ctrl: &mut MainController,
        main_ctrl_rc: &Rc<RefCell<MainController>>,
        app: &gtk::Application,
        window: &gtk::ApplicationWindow,
        builder: &gtk::Builder,
        mut ui_event_receiver: async_mpsc::UnboundedReceiver<UIEvent>,
    ) {
        let mut handler = MainDispatcher {
            app: app.clone(),
            window: window.clone(),
            main_ctrl: Rc::clone(&main_ctrl_rc),
            info_bar_ctrl: InfoBarController::new(app, builder, main_ctrl.ui_event()),
            saved_context: None,
            focus: UIFocusContext::PlaybackPage,
        };

        spawn(async move {
            while let Some(event) = ui_event_receiver.next().await {
                debug!("handling event {:?}", event);
                if handler.handle(event).await.is_err() {
                    break;
                }
            }
        });

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
        quit.connect_activate(
            clone!(@strong main_ctrl.ui_event as ui_event => move |_, _| {
                ui_event.quit();
            }),
        );
        app.set_accels_for_action("app.quit", &["<Ctrl>Q"]);
        app_section.append(Some(&gettext("Quit")), Some("app.quit"));

        main_ctrl.window_delete_id = Some(main_ctrl.window.connect_delete_event(
            clone!(@strong main_ctrl.ui_event as ui_event => move |_, _| {
                ui_event.quit();
                Inhibit(true)
            }),
        ));

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

impl MainDispatcher {
    async fn handle(&mut self, event: UIEvent) -> Result<(), ()> {
        use UIEvent::*;

        match event {
            CancelSelectMedia => self.main_ctrl.borrow_mut().cancel_select_media(),
            Eos => self.main_ctrl.borrow_mut().eos(),
            HideInfoBar => self.info_bar_ctrl.hide(),
            OpenMedia(path) => self.main_ctrl.borrow_mut().open_media(path).await,
            PlayPause => self.main_ctrl.borrow_mut().play_pause().await,
            Quit => {
                self.main_ctrl.borrow_mut().quit();
                return Err(());
            }
            ResetCursor => self.reset_cursor(),
            RestoreContext => self.restore_context(),
            ShowAll => self.show_all(),
            Seek { target, flags } => {
                let _ = self.main_ctrl.borrow_mut().seek(target, flags).await;
            }
            SelectMedia => self.main_ctrl.borrow_mut().select_media().await,
            SetCursorWaiting => self.set_cursor_waiting(),
            ShowError(msg) => self.info_bar_ctrl.show_error(&msg),
            ShowInfo(msg) => self.info_bar_ctrl.show_info(&msg),
            SwitchTo(focus_ctx) => self.switch_to(focus_ctx),
            TemporarilySwitchTo(focus_ctx) => {
                self.save_context();
                self.bind_accels_for(focus_ctx);
            }
            UpdateFocus => self.update_focus(),
        }

        Ok(())
    }

    pub fn show_all(&self) {
        self.window.show();
        self.window.activate();
    }

    fn set_cursor_waiting(&self) {
        if let Some(gdk_window) = self.window.get_window() {
            gdk_window.set_cursor(Some(&Cursor::new_for_display(
                &gdk_window.get_display(),
                CursorType::Watch,
            )));
        }
    }

    fn reset_cursor(&self) {
        if let Some(gdk_window) = self.window.get_window() {
            gdk_window.set_cursor(None);
        }
    }

    fn bind_accels_for(&self, ctx: UIFocusContext) {
        match ctx {
            UIFocusContext::PlaybackPage => {
                self.app
                    .set_accels_for_action("app.play_pause", &["space", "AudioPlay"]);
                self.app
                    .set_accels_for_action("app.next_chapter", &["Down", "AudioNext"]);
                self.app
                    .set_accels_for_action("app.previous_chapter", &["Up", "AudioPrev"]);
                self.app.set_accels_for_action("app.close_info_bar", &[]);
            }
            UIFocusContext::StreamsPage => {
                self.app
                    .set_accels_for_action("app.play_pause", &["space", "AudioPlay"]);
                self.app
                    .set_accels_for_action("app.next_chapter", &["AudioNext"]);
                self.app
                    .set_accels_for_action("app.previous_chapter", &["AudioPrev"]);
                self.app.set_accels_for_action("app.close_info_bar", &[]);
            }
            UIFocusContext::InfoBar => {
                self.app
                    .set_accels_for_action("app.play_pause", &["AudioPlay"]);
                self.app.set_accels_for_action("app.next_chapter", &[]);
                self.app.set_accels_for_action("app.previous_chapter", &[]);
                self.app
                    .set_accels_for_action("app.close_info_bar", &["Escape"]);
            }
        }

        PerspectiveDispatcher::bind_accels_for(ctx, &self.app);
        VideoDispatcher::bind_accels_for(ctx, &self.app);
        InfoDispatcher::bind_accels_for(ctx, &self.app);
        StreamsDispatcher::bind_accels_for(ctx, &self.app);
    }

    fn update_focus(&self) {
        let main_ctrl = self.main_ctrl.borrow();
        match self.focus {
            UIFocusContext::PlaybackPage => main_ctrl.info_ctrl.grab_focus(),
            UIFocusContext::StreamsPage => main_ctrl.streams_ctrl.grab_focus(),
            _ => (),
        }
    }

    fn switch_to(&mut self, ctx: UIFocusContext) {
        self.focus = ctx;
        self.bind_accels_for(ctx);
        self.update_focus();
    }

    fn save_context(&mut self) {
        self.saved_context = Some(self.focus);
    }

    fn restore_context(&mut self) {
        if let Some(focus_ctx) = self.saved_context.take() {
            self.switch_to(focus_ctx);
        }
    }
}
