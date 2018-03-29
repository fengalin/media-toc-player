use gettextrs::gettext;
use glib;
use gstreamer as gst;
use gtk;
use gtk::prelude::*;

use gdk::{Cursor, CursorType, WindowExt};

use std::rc::Rc;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use media::{ContextMessage, PlaybackContext};
use media::ContextMessage::*;

use super::{InfoController, PerspectiveController, StreamsController, VideoController};

const PAUSE_ICON: &str = "media-playback-pause-symbolic";
const PLAYBACK_ICON: &str = "media-playback-start-symbolic";

#[derive(Clone, PartialEq)]
pub enum ControllerState {
    EOS,
    Paused,
    Playing,
    Stopped,
}

const LISTENER_PERIOD: u32 = 250; // 250 ms ( 4 Hz)
const TRACKER_PERIOD: u32 = 40; //  40 ms (25 Hz)

pub struct MainController {
    window: gtk::ApplicationWindow,
    header_bar: gtk::HeaderBar,
    open_btn: gtk::Button,
    play_pause_btn: gtk::ToolButton,
    info_bar: gtk::InfoBar,
    info_bar_lbl: gtk::Label,

    perspective_ctrl: Rc<RefCell<PerspectiveController>>,
    video_ctrl: VideoController,
    info_ctrl: Rc<RefCell<InfoController>>,
    streams_ctrl: Rc<RefCell<StreamsController>>,

    context: Option<PlaybackContext>,
    state: ControllerState,
    seeking: bool,

    this_opt: Option<Rc<RefCell<MainController>>>,
    keep_going: bool,
    listener_src: Option<glib::SourceId>,
    tracker_src: Option<glib::SourceId>,
}

impl MainController {
    pub fn new(builder: &gtk::Builder, is_gst_ok: bool) -> Rc<RefCell<Self>> {
        let this = Rc::new(RefCell::new(MainController {
            window: builder.get_object("application-window").unwrap(),
            header_bar: builder.get_object("header-bar").unwrap(),
            open_btn: builder.get_object("open-btn").unwrap(),
            play_pause_btn: builder.get_object("play_pause-toolbutton").unwrap(),
            info_bar: builder.get_object("info-bar").unwrap(),
            info_bar_lbl: builder.get_object("info_bar-lbl").unwrap(),

            perspective_ctrl: PerspectiveController::new(builder),
            video_ctrl: VideoController::new(builder),
            info_ctrl: InfoController::new(builder),
            streams_ctrl: StreamsController::new(builder),

            context: None,
            state: ControllerState::Stopped,
            seeking: false,

            this_opt: None,
            keep_going: true,
            listener_src: None,
            tracker_src: None,
        }));

        {
            let mut this_mut = this.borrow_mut();

            let this_rc = Rc::clone(&this);
            this_mut.this_opt = Some(this_rc);

            this_mut.window.connect_delete_event(|_, _| {
                gtk::main_quit();
                Inhibit(false)
            });
            this_mut
                .header_bar
                .set_title(gettext("media-toc player").as_str());

            if is_gst_ok {
                this_mut
                    .info_bar
                    .connect_response(|info_bar, _| info_bar.hide());

                this_mut.video_ctrl.register_callbacks(&this);
                PerspectiveController::register_callbacks(&this_mut.perspective_ctrl, &this);
                InfoController::register_callbacks(&this_mut.info_ctrl, &this);
                StreamsController::register_callbacks(&this_mut.streams_ctrl, &this);

                let mut req_err = PlaybackContext::check_requirements().err();
                if req_err.is_some() {
                    let err = req_err.take().unwrap();
                    error!("{}", err);
                    let this_rc = Rc::clone(&this);
                    gtk::idle_add(move || {
                        this_rc
                            .borrow()
                            .show_message(gtk::MessageType::Warning, &err);
                        glib::Continue(false)
                    });
                }

                let this_rc = Rc::clone(&this);
                this_mut.open_btn.connect_clicked(move |_| {
                    this_rc.borrow_mut().select_media();
                });
                this_mut.open_btn.set_sensitive(true);

                let this_rc = Rc::clone(&this);
                this_mut.play_pause_btn.connect_clicked(move |_| {
                    this_rc.borrow_mut().play_pause();
                });
                this_mut.play_pause_btn.set_sensitive(true);

            // TODO: add key bindings to seek by steps
            // play/pause, etc.
            } else {
                // GStreamer initialization failed
                this_mut.info_bar.connect_response(|_, _| gtk::main_quit());

                let msg = gettext("Failed to initialize GStreamer, the application can't be used.");
                this_mut.show_message(gtk::MessageType::Error, &msg);
                error!("{}", msg);
            }
        }

        this
    }

    pub fn show_all(&self) {
        self.window.show_all();
    }

    pub fn show_message(&self, type_: gtk::MessageType, message: &str) {
        self.info_bar.set_message_type(type_);
        self.info_bar_lbl.set_label(message);
        self.info_bar.show();
        // workaround see: https://bugzilla.gnome.org/show_bug.cgi?id=710888
        self.info_bar.queue_resize();
    }

    pub fn play_pause(&mut self) {
        let context = match self.context.take() {
            Some(context) => context,
            None => {
                self.select_media();
                return;
            }
        };

        if self.state != ControllerState::EOS {
            match context.get_state() {
                gst::State::Paused => {
                    self.register_tracker();
                    self.play_pause_btn.set_icon_name(PAUSE_ICON);
                    self.state = ControllerState::Playing;
                    context.play().unwrap();
                    self.context = Some(context);
                }
                gst::State::Playing => {
                    context.pause().unwrap();
                    self.play_pause_btn.set_icon_name(PLAYBACK_ICON);
                    self.remove_tracker();
                    self.state = ControllerState::Paused;
                    self.context = Some(context);
                }
                state => {
                    warn!("Can't play/pause in state {:?}", state);
                    self.context = Some(context);
                }
            };
        } else {
            // Restart the stream from the begining
            self.context = Some(context);
            self.seek(0, true); // accurate (slow)
        }
    }

    pub fn seek(&mut self, position: u64, accurate: bool) {
        if self.state != ControllerState::Stopped {
            self.seeking = true;

            if self.state == ControllerState::Playing || self.state == ControllerState::Paused {
                self.info_ctrl.borrow_mut().seek(position, &self.state);
            }

            self.context.as_ref().unwrap().seek(position, accurate);

            if self.state == ControllerState::EOS {
                self.register_tracker();
                self.play_pause_btn.set_icon_name(PAUSE_ICON);
                self.state = ControllerState::Playing;
            }
        }
    }

    pub fn select_streams(&mut self, stream_ids: &[String]) {
        self.context.as_ref().unwrap().select_streams(stream_ids);
    }

    fn switch_to_busy(&mut self) {
        self.window.set_sensitive(false);

        let gdk_window = self.window.get_window().unwrap();
        gdk_window.set_cursor(&Cursor::new_for_display(
            &gdk_window.get_display(),
            CursorType::Watch,
        ));
    }

    fn switch_to_default(&mut self) {
        self.window.get_window().unwrap().set_cursor(None);
        self.window.set_sensitive(true);
    }

    fn select_media(&mut self) {
        self.switch_to_busy();
        self.info_bar.hide();
        self.remove_tracker();
        self.play_pause_btn.set_icon_name(PLAYBACK_ICON);

        if let Some(context) = self.context.as_mut() {
            context.pause().unwrap();
        };

        let file_dlg = gtk::FileChooserDialog::new(
            Some(&gettext("Open a media file")),
            Some(&self.window),
            gtk::FileChooserAction::Open,
        );
        // Note: couldn't find equivalents for STOCK_OK
        file_dlg.add_button(&gettext("Open"), gtk::ResponseType::Ok.into());

        if file_dlg.run() == gtk::ResponseType::Ok.into() {
            if let Some(ref context) = self.context {
                context.stop();
            }
            self.open_media(file_dlg.get_filename().unwrap());
        } else {
            if self.context.is_some() {
                self.state = ControllerState::Paused;
            }
            self.switch_to_default();
        }

        file_dlg.close();
    }

    pub fn set_context(&mut self, context: PlaybackContext) {
        self.context = Some(context);
        self.state = ControllerState::Paused;
        self.switch_to_default();
    }

    fn remove_listener(&mut self) {
        if let Some(source_id) = self.listener_src.take() {
            glib::source_remove(source_id);
        }
    }

    fn register_listener(&mut self, timeout: u32, ui_rx: Receiver<ContextMessage>) {
        if self.listener_src.is_some() {
            return;
        }

        let this_rc = Rc::clone(self.this_opt.as_ref().unwrap());

        self.listener_src = Some(gtk::timeout_add(timeout, move || {
            let mut keep_going = true;

            for message in ui_rx.try_iter() {
                match message {
                    AsyncDone => {
                        if let Ok(mut this) = this_rc.try_borrow_mut() {
                            this.seeking = false;
                        }
                    }
                    InitDone => {
                        let mut this = this_rc.borrow_mut();

                        let context = this.context.take().unwrap();
                        this.header_bar
                            .set_subtitle(Some(context.file_name.as_str()));
                        this.perspective_ctrl.borrow().new_media();
                        this.streams_ctrl.borrow_mut().new_media(&context);
                        this.info_ctrl.borrow_mut().new_media(&context);
                        this.video_ctrl.new_media(&context);

                        this.register_tracker();
                        this.play_pause_btn.set_icon_name(PAUSE_ICON);
                        this.state = ControllerState::Playing;
                        this.set_context(context);
                    }
                    Eos => {
                        let mut this = this_rc.borrow_mut();
                        let position = this.context.as_mut().unwrap().get_position();
                        this.info_ctrl.borrow_mut().tick(position, true);

                        this.play_pause_btn.set_icon_name(PLAYBACK_ICON);
                        this.state = ControllerState::EOS;

                        // The tracker will be register again in case of a seek
                        this.remove_tracker();
                    }
                    StreamsSelected => {
                        let mut this = this_rc.borrow_mut();
                        let mut context = this.context.take().unwrap();
                        {
                            let info = context.info.lock().unwrap();
                            this.info_ctrl.borrow().streams_changed(&info);
                        }
                        this.set_context(context);
                    }
                    FailedToOpenMedia(error) => {
                        let mut this = this_rc.borrow_mut();
                        this.context = None;
                        this.state = ControllerState::Stopped;
                        this.switch_to_default();

                        this.show_message(gtk::MessageType::Error, &error);

                        this.keep_going = false;
                        keep_going = false;

                        let error = gettext("Error opening file. {}").replacen("{}", &error, 1);
                        this.show_message(gtk::MessageType::Error, &error);
                        error!("{}", error);
                    }
                };

                if !keep_going {
                    break;
                }
            }

            if !keep_going {
                let mut this = this_rc.borrow_mut();
                this.listener_src = None;
                this.tracker_src = None;
            }

            glib::Continue(keep_going)
        }));
    }

    fn remove_tracker(&mut self) {
        if let Some(source_id) = self.tracker_src.take() {
            glib::source_remove(source_id);
        }
    }

    fn register_tracker(&mut self) {
        if self.tracker_src.is_some() {
            return;
        }

        let this_rc = Rc::clone(self.this_opt.as_ref().unwrap());

        self.tracker_src = Some(gtk::timeout_add(TRACKER_PERIOD, move || {
            let mut this = this_rc.borrow_mut();

            if !this.seeking {
                let position = this.context.as_mut().unwrap().get_position();
                this.info_ctrl.borrow_mut().tick(position, false);
            }

            glib::Continue(this.keep_going)
        }));
    }

    pub fn open_media(&mut self, filepath: PathBuf) {
        self.remove_listener();

        self.video_ctrl.cleanup();
        self.info_ctrl.borrow_mut().cleanup();
        self.streams_ctrl.borrow_mut().cleanup();
        self.perspective_ctrl.borrow().cleanup();
        self.header_bar.set_subtitle("");

        let (ctx_tx, ui_rx) = channel();

        self.state = ControllerState::Stopped;
        self.seeking = false;
        self.keep_going = true;
        self.register_listener(LISTENER_PERIOD, ui_rx);

        match PlaybackContext::new(filepath, ctx_tx) {
            Ok(context) => {
                self.context = Some(context);
            }
            Err(error) => {
                self.switch_to_default();
                let error = gettext("Error opening file. {}").replace("{}", &error);
                self.show_message(gtk::MessageType::Error, &error);
                error!("{}", error);
            }
        };
    }
}
