use futures::future::{abortable, AbortHandle, LocalBoxFuture};
use futures::prelude::*;

use gettextrs::{gettext, ngettext};

use gstreamer as gst;
use gtk::prelude::*;

use log::error;

use std::{borrow::ToOwned, cell::RefCell, path::PathBuf, rc::Rc, sync::Arc};

use crate::{
    application::{CommandLineArguments, APP_ID, APP_PATH, CONFIG},
    media::{
        MediaMessage, MissingPlugins, PlaybackPipeline, SeekError, SelectStreamsError, Timestamp,
    },
};

use super::{
    spawn, ui_event, InfoController, MainDispatcher, PerspectiveController, StreamsController,
    UIController, UIEventSender, VideoController,
};

const PAUSE_ICON: &str = "media-playback-pause-symbolic";
const PLAYBACK_ICON: &str = "media-playback-start-symbolic";

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ControllerState {
    EosPaused,
    EosPlaying,
    Paused,
    PendingSelectMediaDecision,
    Playing,
    Stopped,
}

pub struct MainController {
    pub(super) window: gtk::ApplicationWindow,
    pub(super) window_delete_id: Option<glib::signal::SignalHandlerId>,

    header_bar: gtk::HeaderBar,
    pub(super) open_btn: gtk::Button,
    pub(super) display_page: gtk::Box,
    pub(super) play_pause_btn: gtk::ToolButton,
    file_dlg: gtk::FileChooserNative,

    pub(super) ui_event: UIEventSender,

    pub(super) perspective_ctrl: PerspectiveController,
    pub(super) video_ctrl: VideoController,
    pub(super) info_ctrl: InfoController,
    pub(super) streams_ctrl: StreamsController,

    pub(super) pipeline: Option<PlaybackPipeline>,
    pub(super) state: ControllerState,

    media_msg_abort_handle: Option<AbortHandle>,

    pub(super) new_tracker: Option<Box<dyn Fn() -> LocalBoxFuture<'static, ()>>>,
    tracker_abort_handle: Option<AbortHandle>,
}

impl MainController {
    pub fn setup(app: &gtk::Application, args: &CommandLineArguments) {
        let builder =
            gtk::Builder::from_resource(&format!("{}/{}", *APP_PATH, "media-toc-player.ui"));

        let window: gtk::ApplicationWindow = builder.get_object("application-window").unwrap();
        window.set_application(Some(app));

        let (ui_event, ui_event_receiver) = ui_event::new_pair();

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
            window: window.clone(),
            window_delete_id: None,

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
            state: ControllerState::Stopped,

            media_msg_abort_handle: None,

            new_tracker: None,
            tracker_abort_handle: None,
        }));

        let mut main_ctrl = main_ctrl_rc.borrow_mut();
        MainDispatcher::setup(
            &mut main_ctrl,
            &main_ctrl_rc,
            app,
            &window,
            &builder,
            ui_event_receiver,
        );

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
        dialog.set_copyright(Some(&"© 2017–2020 François Laignel"));
        dialog.set_translator_credits(Some(&gettext("translator-credits")));
        dialog.set_license_type(gtk::License::MitX11);
        dialog.set_version(Some(env!("CARGO_PKG_VERSION")));
        dialog.set_website(Some(env!("CARGO_PKG_HOMEPAGE")));
        dialog.set_website_label(Some(&gettext("Learn more about media-toc-player")));

        dialog.connect_response(|dialog, _| dialog.close());
        dialog.show();
    }

    pub fn quit(&mut self) {
        self.abort_tracker();

        if let Some(mut pipeline) = self.pipeline.take() {
            let _ = pipeline.stop();
        }

        if let Some(window_delete_id) = self.window_delete_id.take() {
            let size = self.window.get_size();
            let mut config = CONFIG.write().unwrap();
            config.ui.width = size.0;
            config.ui.height = size.1;
            config.save();

            // Restore default delete handler
            glib::signal::signal_handler_disconnect(&self.window, window_delete_id);
        }

        self.window.close();
    }

    pub async fn play_pause(&mut self) {
        use ControllerState::*;

        match self.state {
            Paused => {
                self.play_pause_btn.set_icon_name(Some(PAUSE_ICON));
                self.state = Playing;
                self.pipeline.as_mut().unwrap().play().await.unwrap();

                self.spawn_tracker();
            }
            Playing => {
                self.pipeline.as_mut().unwrap().pause().await.unwrap();
                self.play_pause_btn.set_icon_name(Some(PLAYBACK_ICON));
                self.abort_tracker();
                self.state = Paused;
            }
            EosPlaying | EosPaused => {
                // Restart the stream from the begining
                self.play_pause_btn.set_icon_name(Some(PAUSE_ICON));
                self.state = Playing;

                if self
                    .seek(Timestamp::default(), gst::SeekFlags::ACCURATE)
                    .await
                    .is_ok()
                {
                    self.pipeline.as_mut().unwrap().play().await.unwrap();
                    self.spawn_tracker();
                }
            }
            Stopped => self.select_media().await,
            PendingSelectMediaDecision => (),
        }
    }

    pub async fn seek(&mut self, position: Timestamp, flags: gst::SeekFlags) -> Result<(), ()> {
        use ControllerState::*;

        match self.state {
            Playing | Paused | EosPaused | EosPlaying => {
                match self.pipeline.as_mut().unwrap().seek(position, flags).await {
                    Ok(()) => {
                        self.info_ctrl.seek(position, self.state);

                        match self.state {
                            EosPlaying => self.state = Playing,
                            EosPaused => self.state = Paused,
                            _ => (),
                        }
                    }
                    Err(SeekError::Eos) => {
                        self.info_ctrl.seek(position, self.state);
                        self.ui_event.eos();
                    }
                    Err(SeekError::Unrecoverable) => {
                        self.stop();
                        return Err(());
                    }
                }
            }
            _ => (),
        }

        Ok(())
    }

    pub fn current_ts(&mut self) -> Option<Timestamp> {
        self.pipeline.as_mut().unwrap().current_ts()
    }

    pub fn tick(&mut self) {
        if let Some(ts) = self.current_ts() {
            self.info_ctrl.tick(ts, self.state);
        }
    }

    pub async fn select_streams(&mut self, stream_ids: &[Arc<str>]) {
        let res = self
            .pipeline
            .as_mut()
            .unwrap()
            .select_streams(stream_ids)
            .await;

        match res {
            Ok(()) => self.streams_selected(),
            Err(SelectStreamsError::Unrecoverable) => self.stop(),
            Err(err) => panic!("{}", err),
        }
    }

    pub fn streams_selected(&mut self) {
        let info = &self.pipeline.as_ref().unwrap().info;
        self.info_ctrl.streams_changed(info);
        self.perspective_ctrl.streams_changed(info);
        self.video_ctrl.streams_changed(info);
    }

    pub fn eos(&mut self) {
        self.play_pause_btn.set_icon_name(Some(PLAYBACK_ICON));

        use ControllerState::*;
        match self.state {
            Playing => self.state = EosPlaying,
            Paused => self.state = EosPaused,
            _ => (),
        }

        self.abort_tracker();
    }

    fn spawn_tracker(&mut self) {
        if self.tracker_abort_handle.is_some() {
            return;
        }

        let (abortable_tracker, abort_handle) = abortable(self.new_tracker.as_ref().unwrap()());
        spawn(abortable_tracker.map(drop));
        self.tracker_abort_handle = Some(abort_handle);
    }

    fn abort_tracker(&mut self) {
        if let Some(abort_handle) = self.tracker_abort_handle.take() {
            abort_handle.abort();
        }
    }

    pub async fn hold(&mut self) {
        self.ui_event.set_cursor_waiting();
        self.play_pause_btn.set_icon_name(Some(PLAYBACK_ICON));

        if let Some(pipeline) = self.pipeline.as_mut() {
            pipeline.pause().await.unwrap();
        };
    }

    pub async fn select_media(&mut self) {
        self.abort_tracker();

        if let ControllerState::Playing | ControllerState::EosPlaying = self.state {
            self.hold().await;
        }

        self.state = ControllerState::PendingSelectMediaDecision;
        self.ui_event.hide_info_bar();

        if let Some(ref last_path) = CONFIG.read().unwrap().media.last_path {
            self.file_dlg.set_current_folder(last_path);
        }
        self.file_dlg.show();
    }

    pub fn stop(&mut self) {
        self.abort_tracker();

        if let Some(mut pipeline) = self.pipeline.take() {
            let _ = pipeline.stop();
        }

        self.state = ControllerState::Stopped;
    }

    pub async fn open_media(&mut self, path: PathBuf) {
        if let Some(abort_handle) = self.media_msg_abort_handle.take() {
            abort_handle.abort();
        }

        self.stop();

        self.info_ctrl.cleanup();
        self.video_ctrl.cleanup();
        self.streams_ctrl.cleanup();
        self.perspective_ctrl.cleanup();
        self.header_bar.set_subtitle(Some(""));

        CONFIG.write().unwrap().media.last_path = path.parent().map(ToOwned::to_owned);

        match PlaybackPipeline::try_new(path.as_ref(), &self.video_ctrl.video_sink()).await {
            Ok(mut pipeline) => {
                if !pipeline.missing_plugins.is_empty() {
                    self.ui_event
                        .show_info(gettext("Some streams are not usable. {}").replace(
                            "{}",
                            &Self::format_missing_plugins(&pipeline.missing_plugins),
                        ));
                }

                self.header_bar
                    .set_subtitle(Some(pipeline.info.file_name.as_str()));

                self.info_ctrl.new_media(&pipeline);
                self.perspective_ctrl.new_media(&pipeline);
                self.streams_ctrl.new_media(&pipeline);
                self.video_ctrl.new_media(&pipeline);

                let ui_event = self.ui_event.clone();
                let mut media_msg_rx = pipeline.media_msg_rx.take().unwrap();
                let (media_msg_handler, abort_handle) = abortable(async move {
                    while let Some(msg) = media_msg_rx.next().await {
                        match msg {
                            MediaMessage::Eos => ui_event.eos(),
                            MediaMessage::Error(err) => {
                                let err = gettext("An unrecoverable error occured. {}")
                                    .replace("{}", &err);
                                error!("{}", err);
                                ui_event.show_error(err);
                                break;
                            }
                        }
                    }
                });
                self.media_msg_abort_handle = Some(abort_handle);
                spawn(media_msg_handler.map(|_| ()));

                self.pipeline = Some(pipeline);

                self.streams_selected();

                self.ui_event.reset_cursor();
                self.state = ControllerState::Paused;
            }
            Err(error) => {
                use super::media::playback_pipeline::OpenError;

                self.ui_event.reset_cursor();

                let error = match error {
                    OpenError::Generic(error) => error,
                    OpenError::MissingPlugins(plugins) => Self::format_missing_plugins(&plugins),
                    OpenError::StateChange => gettext("Failed to switch the media to Paused"),
                    OpenError::GLSinkError => {
                        let mut config = CONFIG.write().expect("Failed to get CONFIG as mut");
                        config.media.is_gl_disabled = true;
                        config.save();

                        gettext(
        "Video rendering hardware acceleration seems broken and has been disabled.\nPlease restart the application.",
                        )
                    }
                };

                self.ui_event
                    .show_error(gettext("Error opening file. {}").replace("{}", &error));
            }
        };
    }

    fn format_missing_plugins(plugins: &MissingPlugins) -> String {
        ngettext(
            "Missing plugin:\n{}",
            "Missing plugins:\n{}",
            plugins.len() as u32,
        )
        .replacen("{}", &format!("{}", plugins), 1)
    }

    pub fn cancel_select_media(&mut self) {
        if self.state == ControllerState::PendingSelectMediaDecision {
            self.state = if self.pipeline.is_some() {
                ControllerState::Paused
            } else {
                ControllerState::Stopped
            };
        }
    }
}
