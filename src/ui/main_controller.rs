extern crate gdk;
extern crate glib;
extern crate gstreamer as gst;
extern crate gtk;

use std::rc::Rc;
use std::cell::RefCell;

use std::path::PathBuf;

use std::sync::mpsc::{channel, Receiver};

use gdk::{Cursor, CursorType, WindowExt};

use gtk::prelude::*;

use media::{Context, ContextMessage};
use media::ContextMessage::*;

use super::{InfoController, StreamsController, VideoController};

#[derive(Clone, PartialEq)]
pub enum ControllerState {
    EOS,
    Paused,
    Playing,
    Ready,
    Stopped,
}

const LISTENER_PERIOD: u32 = 250; // 250 ms ( 4 Hz)
const TRACKER_PERIOD: u32 = 40; //  40 ms (25 Hz)

pub struct MainController {
    window: gtk::ApplicationWindow,
    header_bar: gtk::HeaderBar,
    play_pause_btn: gtk::ToolButton,

    video_ctrl: VideoController,
    info_ctrl: Rc<RefCell<InfoController>>,
    streams_ctrl: Rc<RefCell<StreamsController>>,

    context: Option<Context>,
    state: ControllerState,
    seeking: bool,

    this_opt: Option<Rc<RefCell<MainController>>>,
    keep_going: bool,
    listener_src: Option<glib::SourceId>,
    tracker_src: Option<glib::SourceId>,
}

impl MainController {
    pub fn new(builder: &gtk::Builder) -> Rc<RefCell<Self>> {
        let this = Rc::new(RefCell::new(MainController {
            window: builder.get_object("application-window").unwrap(),
            header_bar: builder.get_object("header-bar").unwrap(),
            play_pause_btn: builder.get_object("play_pause-toolbutton").unwrap(),

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
            this_mut.window.set_titlebar(&this_mut.header_bar);

            let this_rc = Rc::clone(&this);
            this_mut.play_pause_btn.connect_clicked(move |_| {
                this_rc.borrow_mut().play_pause();
            });

            // TODO: add key bindings to seek by steps
            // play/pause, etc.

            VideoController::register_callbacks(&this_mut.video_ctrl, &this);
            InfoController::register_callbacks(&this_mut.info_ctrl, &this);
            StreamsController::register_callbacks(&this_mut.streams_ctrl, &this);
        }

        let open_btn: gtk::Button = builder.get_object("open-btn").unwrap();
        let this_rc = Rc::clone(&this);
        open_btn.connect_clicked(move |_| {
            this_rc.borrow_mut().select_media();
        });

        this
    }

    pub fn show_all(&self) {
        self.window.show_all();
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
                    self.play_pause_btn.set_icon_name("media-playback-pause");
                    self.state = ControllerState::Playing;
                    context.play().unwrap();
                    self.context = Some(context);
                }
                gst::State::Playing => {
                    context.pause().unwrap();
                    self.play_pause_btn.set_icon_name("media-playback-start");
                    self.remove_tracker();
                    self.state = ControllerState::Paused;
                    self.context = Some(context);
                }
                state => {
                    println!("Can't play/pause in state {:?}", state);
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

            // update position even though the stream
            // is not sync yet for the user to notice
            // the seek request in being handled
            self.info_ctrl.borrow_mut().seek(position);

            self.context
                .as_ref()
                .expect("MainController::seek no context")
                .seek(position, accurate);

            if self.state == ControllerState::EOS || self.state == ControllerState::Ready {
                if self.state == ControllerState::Ready {
                    self.context
                        .as_ref()
                        .expect("MainController::seek no context")
                        .play()
                        .unwrap();
                }
                self.register_tracker();
                self.play_pause_btn.set_icon_name("media-playback-pause");
                self.state = ControllerState::Playing;
            }
        }
    }

    pub fn select_streams(&mut self, stream_ids: &[String]) {
        self.context
            .as_ref()
            .expect("MainController::select_streams no context")
            .select_streams(stream_ids);
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
        self.remove_tracker();
        self.play_pause_btn.set_icon_name("media-playback-start");

        if let Some(context) = self.context.as_mut() {
            context.pause().unwrap();
        };

        let file_dlg = gtk::FileChooserDialog::new(
            Some("Open a media file"),
            Some(&self.window),
            gtk::FileChooserAction::Open,
        );
        // Note: couldn't find equivalents for STOCK_OK
        file_dlg.add_button("Open", gtk::ResponseType::Ok.into());

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

    pub fn set_context(&mut self, context: Context) {
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

                        let context = this.context
                            .take()
                            .expect("MainController: InitDone but no context available");

                        this.header_bar
                            .set_subtitle(Some(context.file_name.as_str()));
                        this.streams_ctrl.borrow_mut().new_media(&context);
                        this.info_ctrl.borrow_mut().new_media(&context);
                        this.video_ctrl.new_media(&context);

                        this.set_context(context);
                        this.state = ControllerState::Ready;
                    }
                    Eos => {
                        let mut this = this_rc.borrow_mut();
                        let position = this.context
                            .as_mut()
                            .expect("MainController::listener no context while getting position")
                            .get_position();

                        this.info_ctrl.borrow_mut().tick(position, true);

                        this.play_pause_btn.set_icon_name("media-playback-start");
                        this.state = ControllerState::EOS;

                        // The tracker will be register again in case of a seek
                        this.remove_tracker();
                    }
                    StreamsSelected => {
                        let mut this = this_rc.borrow_mut();
                        let mut context = this.context
                            .take()
                            .expect("MainController(StreamsSelected) no context available");
                        {
                            let info = context
                                .info
                                .lock()
                                .expect("MainController(StreamsSelected) failed to lock info");

                            this.info_ctrl.borrow().streams_changed(&info);
                        }
                        this.set_context(context);
                    }
                    FailedToOpenMedia => {
                        eprintln!("ERROR: failed to open media");

                        let mut this = this_rc.borrow_mut();
                        this.context = None;
                        this.state = ControllerState::Stopped;
                        this.switch_to_default();

                        this.keep_going = false;
                        keep_going = false;
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
                let position = this.context
                    .as_mut()
                    .expect("MainController::tracker no context while getting position")
                    .get_position();
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
        self.header_bar.set_subtitle("");

        let (ctx_tx, ui_rx) = channel();

        self.state = ControllerState::Stopped;
        self.seeking = false;
        self.keep_going = true;
        self.register_listener(LISTENER_PERIOD, ui_rx);

        match Context::new(filepath, ctx_tx) {
            Ok(context) => {
                self.context = Some(context);
            }
            Err(error) => {
                self.switch_to_default();
                eprintln!("Error opening media: {}", error);
            }
        };
    }
}
