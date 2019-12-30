use futures::channel::mpsc as async_mpsc;
use futures::future::{abortable, AbortHandle, LocalBoxFuture};
use futures::prelude::*;

use gettextrs::{gettext, ngettext};

use glib;
use gstreamer as gst;
use gtk;
use gtk::prelude::*;

use log::{debug, error};

use std::{borrow::ToOwned, cell::RefCell, collections::HashSet, path::PathBuf, rc::Rc, sync::Arc};

use crate::{
    application::{CommandLineArguments, APP_ID, APP_PATH, CONFIG},
    media::{MediaEvent, PlaybackPipeline, PlaybackState, Timestamp},
    spawn,
};

use super::{
    InfoController, MainDispatcher, MediaEventReceiver, PerspectiveController, StreamsController,
    UIController, UIEventHandler, UIEventSender, VideoController,
};

const PAUSE_ICON: &str = "media-playback-pause-symbolic";
const PLAYBACK_ICON: &str = "media-playback-start-symbolic";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControllerState {
    EOS,
    Paused,
    PendingSelectMedia,
    PendingSelectMediaDecision,
    Playing,
    Seeking,
    Stopped,
}

pub struct MainController {
    pub(super) window: gtk::ApplicationWindow,

    header_bar: gtk::HeaderBar,
    pub(super) open_btn: gtk::Button,
    pub(super) display_page: gtk::Box,
    pub(super) play_pause_btn: gtk::ToolButton,
    file_dlg: gtk::FileChooserNative,

    ui_event: UIEventSender,

    pub(super) perspective_ctrl: PerspectiveController,
    pub(super) video_ctrl: VideoController,
    pub(super) info_ctrl: InfoController,
    pub(super) streams_ctrl: StreamsController,

    pub(super) pipeline: Option<PlaybackPipeline>,
    missing_plugins: HashSet<String>,
    pub(super) state: ControllerState,

    pub(super) new_media_event_handler:
        Option<Box<dyn Fn(MediaEventReceiver) -> LocalBoxFuture<'static, ()>>>,
    media_event_abort_handle: Option<AbortHandle>,

    pub(super) new_tracker: Option<Box<dyn Fn() -> LocalBoxFuture<'static, ()>>>,
    tracker_abort_handle: Option<AbortHandle>,
}

impl MainController {
    pub fn setup(app: &gtk::Application, args: &CommandLineArguments) {
        let builder =
            gtk::Builder::new_from_resource(&format!("{}/{}", *APP_PATH, "media-toc-player.ui"));

        let window: gtk::ApplicationWindow = builder.get_object("application-window").unwrap();
        window.set_application(Some(app));

        let (mut ui_event_handler, ui_event) = UIEventHandler::new_pair(&app, &builder);

        let file_dlg = gtk::FileChooserNativeBuilder::new()
            .title(&gettext("Open a media file"))
            .transient_for(&window)
            .modal(true)
            .accept_label(&gettext("Open"))
            .cancel_label(&gettext("Cancel"))
            .build();

        let ui_event_clone = ui_event.clone();
        file_dlg.connect_response(move |file_dlg, response| {
            file_dlg.hide();
            match (response, file_dlg.get_filename()) {
                (gtk::ResponseType::Accept, Some(path)) => ui_event_clone.open_media(path),
                _ => ui_event_clone.cancel_select_media(),
            }
        });

        let gst_init_res = gst::init();

        let main_ctrl_rc = Rc::new(RefCell::new(MainController {
            window,
            header_bar: builder.get_object("header-bar").unwrap(),
            open_btn: builder.get_object("open-btn").unwrap(),
            display_page: builder.get_object("video-container").unwrap(),
            play_pause_btn: builder.get_object("play_pause-toolbutton").unwrap(),
            file_dlg,

            ui_event: ui_event.clone(),

            perspective_ctrl: PerspectiveController::new(&builder),
            video_ctrl: VideoController::new(&builder, args),
            info_ctrl: InfoController::new(&builder, ui_event.clone()),
            streams_ctrl: StreamsController::new(&builder),

            pipeline: None,
            missing_plugins: HashSet::<String>::new(),
            state: ControllerState::Stopped,

            new_media_event_handler: None,
            media_event_abort_handle: None,

            new_tracker: None,
            tracker_abort_handle: None,
        }));

        ui_event_handler.have_main_ctrl(&main_ctrl_rc);
        ui_event_handler.spawn();

        let mut main_ctrl = main_ctrl_rc.borrow_mut();
        MainDispatcher::setup(&mut main_ctrl, &main_ctrl_rc, app);

        if gst_init_res.is_ok() {
            {
                let config = CONFIG.read().unwrap();
                if config.ui.width > 0 && config.ui.height > 0 {
                    main_ctrl.window.resize(config.ui.width, config.ui.height);
                }

                main_ctrl.open_btn.set_sensitive(true);
            }

            ui_event.show_all();

            if let Some(input_file) = args.input_file.to_owned() {
                main_ctrl.ui_event.open_media(input_file);
            }
        } else {
            ui_event.show_all();
        }
    }

    pub fn ui_event(&self) -> &UIEventSender {
        &self.ui_event
    }

    pub fn about(&self) {
        let dialog = gtk::AboutDialog::new();
        dialog.set_modal(true);
        dialog.set_transient_for(Some(&self.window));

        dialog.set_program_name(env!("CARGO_PKG_NAME"));
        dialog.set_logo_icon_name(Some(&APP_ID));
        dialog.set_comments(Some(&gettext("A media player with a table of contents")));
        dialog.set_copyright(Some(&gettext("© 2017–2019 François Laignel")));
        dialog.set_translator_credits(Some(&gettext("translator-credits")));
        dialog.set_license_type(gtk::License::MitX11);
        dialog.set_version(Some(env!("CARGO_PKG_VERSION")));
        dialog.set_website(Some(env!("CARGO_PKG_HOMEPAGE")));
        dialog.set_website_label(Some(&gettext("Learn more about media-toc-player")));

        dialog.connect_response(|dialog, _| dialog.close());
        dialog.show();
    }

    pub fn quit(&mut self) {
        if let Some(pipeline) = self.pipeline.take() {
            pipeline.stop();
        }
        self.abort_media_event_handler();
        self.abort_tracker();

        {
            let size = self.window.get_size();
            let mut config = CONFIG.write().unwrap();
            config.ui.width = size.0;
            config.ui.height = size.1;
            config.save();
        }

        self.window.destroy();
    }

    pub fn play_pause(&mut self) {
        let pipeline = match self.pipeline.as_mut() {
            Some(pipeline) => pipeline,
            None => {
                self.select_media();
                return;
            }
        };

        if self.state != ControllerState::EOS {
            match pipeline.get_state() {
                gst::State::Paused => {
                    self.play_pause_btn.set_icon_name(Some(PAUSE_ICON));
                    self.state = ControllerState::Playing;
                    pipeline.play().unwrap();
                    self.spawn_tracker();
                }
                gst::State::Playing => {
                    pipeline.pause().unwrap();
                    self.play_pause_btn.set_icon_name(Some(PLAYBACK_ICON));
                    self.abort_tracker();
                    self.state = ControllerState::Paused;
                }
                _ => {
                    self.select_media();
                }
            };
        } else {
            // Restart the stream from the begining
            self.seek(Timestamp::default(), gst::SeekFlags::ACCURATE);
        }
    }

    pub fn seek(&mut self, position: Timestamp, flags: gst::SeekFlags) {
        self.info_ctrl.seek(position);
        if self.state == ControllerState::EOS {
            self.spawn_tracker();
        }
        self.state = ControllerState::Seeking;
        self.pipeline.as_ref().unwrap().seek(position, flags);
    }

    pub fn get_current_ts(&mut self) -> Timestamp {
        self.pipeline.as_mut().unwrap().get_current_ts()
    }

    pub fn tick(&mut self) {
        match self.state {
            ControllerState::Seeking => (),
            _ => {
                let ts = self.get_current_ts();
                self.info_ctrl.tick(ts, self.state);
            }
        }
    }

    pub fn select_streams(&mut self, stream_ids: &[Arc<str>]) {
        self.pipeline.as_ref().unwrap().select_streams(stream_ids);
        // In Playing state, wait for the notification from the pipeline
        // Otherwise, update immediately
        if self.state != ControllerState::Playing {
            self.streams_selected();
        }
    }

    pub fn streams_selected(&mut self) {
        let info = self.pipeline.as_ref().unwrap().info.read().unwrap();
        self.info_ctrl.streams_changed(&info);
        self.perspective_ctrl.streams_changed(&info);
        self.video_ctrl.streams_changed(&info);
    }

    pub fn hold(&mut self) {
        self.ui_event.set_cursor_waiting();
        self.play_pause_btn.set_icon_name(Some(PLAYBACK_ICON));

        if let Some(pipeline) = self.pipeline.as_mut() {
            pipeline.pause().unwrap();
        };
    }

    fn check_missing_plugins(&self) -> Option<String> {
        if !self.missing_plugins.is_empty() {
            let mut missing_nb = 0;
            let mut missing_list = String::new();

            self.missing_plugins.iter().for_each(|missing_plugin| {
                if missing_nb > 0 {
                    missing_list += ", ";
                }

                missing_list += missing_plugin;
                missing_nb += 1;
            });
            let message = ngettext("Missing plugin: {}", "Missing plugins: {}", missing_nb)
                .replacen("{}", &missing_list, 1);

            Some(message)
        } else {
            None
        }
    }

    fn spawn_media_event_handler(&mut self) -> async_mpsc::Sender<MediaEvent> {
        let (sender, receiver) = async_mpsc::channel(1);

        let (abortable_event_handler, abort_handle) =
            abortable(self.new_media_event_handler.as_ref().unwrap()(receiver));
        spawn!(abortable_event_handler.map(drop));
        self.media_event_abort_handle = Some(abort_handle);

        sender
    }

    fn abort_media_event_handler(&mut self) {
        if let Some(abort_handle) = self.media_event_abort_handle.take() {
            abort_handle.abort();
        }
    }

    pub fn handle_media_event(&mut self, event: MediaEvent) -> Result<(), ()> {
        let mut keep_going = true;

        match event {
            MediaEvent::AsyncDone(playback_state) => {
                if let ControllerState::Seeking = self.state {
                    self.state = match playback_state {
                        PlaybackState::Playing => ControllerState::Playing,
                        PlaybackState::Paused => ControllerState::Paused,
                    };
                }
            }
            MediaEvent::InitDone => {
                debug!("received `InitDone`");
                {
                    let pipeline = self.pipeline.as_ref().unwrap();

                    self.header_bar
                        .set_subtitle(Some(pipeline.info.read().unwrap().file_name.as_str()));

                    self.info_ctrl.new_media(&pipeline);
                    self.perspective_ctrl.new_media(&pipeline);
                    self.streams_ctrl.new_media(&pipeline);
                    self.video_ctrl.new_media(&pipeline);
                }

                self.streams_selected();

                if let Some(message) = self.check_missing_plugins() {
                    self.ui_event.show_error(message);
                }

                self.ui_event.reset_cursor();
                self.state = ControllerState::Paused;
            }
            MediaEvent::MissingPlugin(plugin) => {
                error!(
                    "{}",
                    gettext("Missing plugin: {}").replacen("{}", &plugin, 1)
                );
                self.missing_plugins.insert(plugin);
            }
            MediaEvent::ReadyToRefresh => match &self.state {
                ControllerState::Playing => (),
                ControllerState::Paused => (),
                ControllerState::PendingSelectMedia => {
                    self.select_media();
                }
                _ => (),
            },
            MediaEvent::StreamsSelected => self.streams_selected(),
            MediaEvent::Eos => {
                self.play_pause_btn.set_icon_name(Some(PLAYBACK_ICON));
                self.state = ControllerState::EOS;
                self.abort_tracker();
            }
            MediaEvent::FailedToOpenMedia(error) => {
                self.pipeline = None;
                self.state = ControllerState::Stopped;
                self.ui_event.reset_cursor();

                keep_going = false;

                let mut error = gettext("Error opening file.\n\n{}").replacen("{}", &error, 1);
                if let Some(message) = self.check_missing_plugins() {
                    error += "\n\n";
                    error += &message;
                }
                self.ui_event.show_error(error);
            }
            MediaEvent::GLSinkError => {
                self.pipeline = None;
                self.state = ControllerState::Stopped;
                self.ui_event.reset_cursor();

                let mut config = CONFIG.write().expect("Failed to get CONFIG as mut");
                config.media.is_gl_disabled = true;
                config.save();

                keep_going = false;

                self.ui_event.show_error(gettext(
"Video rendering hardware acceleration seems broken and has been disabled.\nPlease restart the application.",
                ));
            }
        }

        if keep_going {
            Ok(())
        } else {
            self.abort_tracker();
            Err(())
        }
    }

    fn spawn_tracker(&mut self) {
        if self.tracker_abort_handle.is_some() {
            return;
        }

        let (abortable_tracker, abort_handle) = abortable(self.new_tracker.as_ref().unwrap()());
        spawn!(abortable_tracker.map(drop));
        self.tracker_abort_handle = Some(abort_handle);
    }

    fn abort_tracker(&mut self) {
        if let Some(abort_handle) = self.tracker_abort_handle.take() {
            abort_handle.abort();
        }
    }

    pub fn select_media(&mut self) {
        self.abort_tracker();
        self.state = ControllerState::PendingSelectMediaDecision;
        self.ui_event.hide_info_bar();

        if let Some(ref last_path) = CONFIG.read().unwrap().media.last_path {
            self.file_dlg.set_current_folder(last_path);
        }
        self.file_dlg.show();
    }

    pub fn open_media(&mut self, path: PathBuf) {
        if let Some(pipeline) = self.pipeline.take() {
            pipeline.stop();
        }

        self.abort_media_event_handler();

        self.info_ctrl.cleanup();
        self.video_ctrl.cleanup();
        self.streams_ctrl.cleanup();
        self.perspective_ctrl.cleanup();
        self.header_bar.set_subtitle(Some(""));

        self.state = ControllerState::Stopped;
        self.missing_plugins.clear();
        let sender = self.spawn_media_event_handler();

        match PlaybackPipeline::try_new(path.as_ref(), &self.video_ctrl.get_video_sink(), sender) {
            Ok(pipeline) => {
                CONFIG.write().unwrap().media.last_path = path.parent().map(ToOwned::to_owned);
                self.pipeline = Some(pipeline);
            }
            Err(error) => {
                self.ui_event.reset_cursor();
                let error = gettext("Error opening file.\n\n{}").replace("{}", &error);
                self.ui_event.show_error(error);
            }
        };
    }

    pub fn cancel_select_media(&mut self) {
        if self.state == ControllerState::PendingSelectMediaDecision {
            self.state = self
                .pipeline
                .as_ref()
                .map_or(ControllerState::Stopped, |_| ControllerState::Paused);
        }
    }
}
