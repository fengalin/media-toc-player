use gdk::{Cursor, CursorType, WindowExt};
use gettextrs::{gettext, ngettext};

use glib;
use gstreamer as gst;
use gtk;
use gtk::prelude::*;

use log::{debug, error, info};

use std::{cell::RefCell, collections::HashSet, path::PathBuf, rc::Rc, sync::Arc};

use crate::{
    application::{CommandLineArguments, APP_ID, APP_PATH, CONFIG},
    media::{MediaEvent, PlaybackPipeline, PlaybackState},
};

use super::{
    InfoController, PerspectiveController, StreamsController, UIController, UIEvent, UIEventSender,
    VideoController,
};

const PAUSE_ICON: &str = "media-playback-pause-symbolic";
const PLAYBACK_ICON: &str = "media-playback-start-symbolic";

const TRACKER_PERIOD: u32 = 40; //  40 ms (25 Hz)

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
    pub(super) ui_event_receiver: Option<glib::Receiver<UIEvent>>,
    ui_event_sender: UIEventSender,

    pub(super) window: gtk::ApplicationWindow,

    header_bar: gtk::HeaderBar,
    pub(super) open_btn: gtk::Button,
    pub(super) play_pause_btn: gtk::ToolButton,
    pub(super) info_bar_revealer: gtk::Revealer,
    pub(super) info_bar: gtk::InfoBar,
    info_bar_lbl: gtk::Label,

    pub(super) perspective_ctrl: PerspectiveController,
    pub(super) video_ctrl: VideoController,
    pub(super) info_ctrl: InfoController,
    pub(super) streams_ctrl: StreamsController,

    pub(super) pipeline: Option<PlaybackPipeline>,
    missing_plugins: HashSet<String>,
    pub(super) state: ControllerState,

    pub(super) select_media_async: Option<Box<Fn()>>,
    pub(super) media_event_handler: Option<Rc<Fn(MediaEvent) -> glib::Continue>>,
    media_event_handler_src: Option<glib::SourceId>,

    pub(super) tracker_fn: Option<Rc<Fn()>>,
    tracker_src: Option<glib::SourceId>,
}

impl MainController {
    pub fn new_rc() -> Rc<RefCell<Self>> {
        let builder =
            gtk::Builder::new_from_resource(&format!("{}/{}", *APP_PATH, "media-toc-player.ui"));
        let (ui_event_sender, ui_event_receiver) =
            glib::MainContext::channel(glib::PRIORITY_DEFAULT);
        let ui_event_sender: UIEventSender = ui_event_sender.into();

        Rc::new(RefCell::new(MainController {
            ui_event_receiver: Some(ui_event_receiver),
            ui_event_sender: ui_event_sender.clone(),

            window: builder.get_object("application-window").unwrap(),
            header_bar: builder.get_object("header-bar").unwrap(),
            open_btn: builder.get_object("open-btn").unwrap(),
            play_pause_btn: builder.get_object("play_pause-toolbutton").unwrap(),
            info_bar_revealer: builder.get_object("info_bar-revealer").unwrap(),
            info_bar: builder.get_object("info_bar").unwrap(),
            info_bar_lbl: builder.get_object("info_bar-lbl").unwrap(),

            perspective_ctrl: PerspectiveController::new(&builder),
            video_ctrl: VideoController::new(&builder),
            info_ctrl: InfoController::new(&builder, ui_event_sender.clone()),
            streams_ctrl: StreamsController::new(&builder),

            pipeline: None,
            missing_plugins: HashSet::<String>::new(),
            state: ControllerState::Stopped,

            select_media_async: None,
            media_event_handler: None,
            media_event_handler_src: None,

            tracker_fn: None,
            tracker_src: None,
        }))
    }

    pub fn setup(&mut self, args: &CommandLineArguments) {
        if gst::init().is_ok() {
            {
                let config = CONFIG.read().unwrap();
                if config.ui.width > 0 && config.ui.height > 0 {
                    self.window.resize(config.ui.width, config.ui.height);
                }

                self.open_btn.set_sensitive(true);
            }

            self.perspective_ctrl.setup(&args);
            self.video_ctrl.setup(&args);
            self.info_ctrl.setup(&args);
            self.streams_ctrl.setup(&args);
        }
    }

    pub fn get_ui_event_sender(&self) -> UIEventSender {
        self.ui_event_sender.clone()
    }

    pub fn show_all(&self) {
        self.window.show();
        self.window.activate();
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
        self.remove_media_event_handler();
        self.remove_tracker();

        {
            let size = self.window.get_size();
            let mut config = CONFIG.write().unwrap();
            config.ui.width = size.0;
            config.ui.height = size.1;
            config.save();
        }

        self.window.destroy();
    }

    pub fn show_message<Msg: AsRef<str>>(&mut self, type_: gtk::MessageType, message: Msg) {
        self.info_bar.set_message_type(type_);
        self.info_bar_lbl.set_label(message.as_ref());
        self.info_bar_revealer.set_reveal_child(true);
    }

    pub fn show_error<Msg: AsRef<str>>(&mut self, message: Msg) {
        error!("{}", message.as_ref());
        self.show_message(gtk::MessageType::Error, message);
    }

    pub fn show_info<Msg: AsRef<str>>(&mut self, message: Msg) {
        info!("{}", message.as_ref());
        self.show_message(gtk::MessageType::Info, message);
    }

    pub fn play_pause(&mut self) {
        let pipeline_state = match &self.pipeline {
            Some(pipeline) => pipeline.get_state(),
            None => {
                self.select_media();
                return;
            }
        };

        if self.state != ControllerState::EOS {
            match pipeline_state {
                gst::State::Paused => {
                    self.play_pause_btn.set_icon_name(Some(PAUSE_ICON));
                    self.state = ControllerState::Playing;
                    self.register_tracker();
                    self.pipeline.as_mut().unwrap().play().unwrap();
                }
                gst::State::Playing => {
                    self.pipeline.as_mut().unwrap().pause().unwrap();
                    self.play_pause_btn.set_icon_name(Some(PLAYBACK_ICON));
                    self.remove_tracker();
                    self.state = ControllerState::Paused;
                }
                _ => {
                    self.select_media();
                }
            };
        } else {
            // Restart the stream from the begining
            self.seek(0, gst::SeekFlags::ACCURATE);
        }
    }

    pub fn seek(&mut self, position: u64, flags: gst::SeekFlags) {
        self.info_ctrl.seek(position, &self.state);
        if self.state == ControllerState::EOS {
            self.register_tracker();
        }
        self.state = ControllerState::Seeking;
        self.pipeline.as_ref().unwrap().seek(position, flags);
    }

    pub fn get_position(&mut self) -> u64 {
        self.pipeline.as_mut().unwrap().get_position()
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
        self.set_cursor_waiting();
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

    #[allow(clippy::redundant_closure)]
    fn attach_media_event_handler(&mut self, receiver: glib::Receiver<MediaEvent>) {
        let media_event_handler = Rc::clone(self.media_event_handler.as_ref().unwrap());
        self.media_event_handler_src =
            Some(receiver.attach(None, move |event| media_event_handler(event)));
    }

    fn remove_media_event_handler(&mut self) {
        if let Some(source_id) = self.media_event_handler_src.take() {
            glib::source_remove(source_id);
        }
    }

    pub fn handle_media_event(&mut self, event: MediaEvent) -> glib::Continue {
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
                    self.show_error(message);
                }

                self.reset_cursor();
                self.state = ControllerState::Paused;
            }
            MediaEvent::MissingPlugin(plugin) => {
                error!(
                    "{}",
                    gettext("Missing plugin: {}").replacen("{}", &plugin, 1)
                );
                self.missing_plugins.insert(plugin);
            }
            MediaEvent::ReadyForRefresh => match &self.state {
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
                // The tracker will be register again in case of a seek
                self.remove_tracker();
            }
            MediaEvent::FailedToOpenMedia(error) => {
                self.pipeline = None;
                self.state = ControllerState::Stopped;
                self.reset_cursor();

                keep_going = false;

                let mut error = gettext("Error opening file.\n\n{}").replacen("{}", &error, 1);
                if let Some(message) = self.check_missing_plugins() {
                    error += "\n\n";
                    error += &message;
                }
                self.show_error(error);
            }
        }

        if !keep_going {
            self.remove_media_event_handler();
            self.remove_tracker();
        }

        glib::Continue(keep_going)
    }

    pub fn set_cursor_waiting(&self) {
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

    pub fn select_media(&mut self) {
        self.info_bar_revealer.set_reveal_child(false);
        self.remove_tracker();
        self.state = ControllerState::PendingSelectMediaDecision;
        self.select_media_async.as_ref().unwrap()();
    }

    fn remove_tracker(&mut self) {
        if let Some(source_id) = self.tracker_src.take() {
            glib::source_remove(source_id);
            // Call the tracker one last time to update the UI
            let tracker_fn = Rc::clone(self.tracker_fn.as_ref().unwrap());
            gtk::idle_add(move || {
                tracker_fn();
                glib::Continue(false)
            });
        }
    }

    fn register_tracker(&mut self) {
        if self.tracker_src.is_some() {
            return;
        }

        let tracker_fn = Rc::clone(self.tracker_fn.as_ref().unwrap());
        self.tracker_src = Some(gtk::timeout_add(TRACKER_PERIOD, move || {
            tracker_fn();
            glib::Continue(true)
        }));
    }

    pub fn open_media(&mut self, path: PathBuf) {
        if let Some(pipeline) = self.pipeline.take() {
            pipeline.stop();
        }

        self.remove_media_event_handler();

        self.info_ctrl.cleanup();
        self.video_ctrl.cleanup();
        self.streams_ctrl.cleanup();
        self.perspective_ctrl.cleanup();
        self.header_bar.set_subtitle(Some(""));

        let (sender, receiver) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);

        self.state = ControllerState::Stopped;
        self.missing_plugins.clear();
        self.attach_media_event_handler(receiver);

        match PlaybackPipeline::try_new(path.as_ref(), &self.video_ctrl.get_video_sink(), sender) {
            Ok(pipeline) => {
                CONFIG.write().unwrap().media.last_path =
                    path.parent().map(|parent_path| parent_path.to_owned());
                self.pipeline = Some(pipeline);
            }
            Err(error) => {
                self.reset_cursor();
                let error = gettext("Error opening file.\n\n{}").replace("{}", &error);
                self.show_error(error);
            }
        };
    }

    pub fn cancel_select_media(&mut self) {
        self.reset_cursor();
        match &self.state {
            ControllerState::PendingSelectMediaDecision => {
                self.state = self
                    .pipeline
                    .as_ref()
                    .map_or(ControllerState::Stopped, |_| ControllerState::Paused);
            }
            other => panic!("Called `cancel_select_media()` in state {:?}", other),
        }
    }
}
