use futures::channel::mpsc as async_mpsc;
use futures::prelude::*;

use gdk::{Cursor, CursorType, WindowExt};
use gstreamer as gst;
use gtk::prelude::*;

use log::debug;

use std::{
    borrow::Cow,
    cell::{Ref, RefCell, RefMut},
    path::PathBuf,
    rc::Rc,
};

use crate::{media::Timestamp, spawn};

use super::{
    InfoBarController, InfoDispatcher, MainController, PerspectiveDispatcher, StreamsDispatcher,
    UIController, UIDispatcher, VideoDispatcher,
};

const UI_EVENT_CHANNEL_CAPACITY: usize = 4;

#[derive(Clone, Copy, Debug)]
pub enum UIFocusContext {
    InfoBar,
    PlaybackPage,
    StreamsPage,
}

#[derive(Debug)]
enum UIEvent {
    CancelSelectMedia,
    HideInfoBar,
    OpenMedia(PathBuf),
    ResetCursor,
    RestoreContext,
    Seek {
        target: Timestamp,
        flags: gst::SeekFlags,
    },
    ShowAll,
    SetCursorWaiting,
    ShowError(Cow<'static, str>),
    ShowInfo(Cow<'static, str>),
    SwitchTo(UIFocusContext),
    TemporarilySwitchTo(UIFocusContext),
    UpdateFocus,
}

#[derive(Clone)]
pub struct UIEventSender(RefCell<async_mpsc::Sender<UIEvent>>);

#[allow(unused_must_use)]
impl UIEventSender {
    fn send(&self, event: UIEvent) {
        self.0.borrow_mut().try_send(event).unwrap();
    }

    pub fn cancel_select_media(&self) {
        self.send(UIEvent::CancelSelectMedia);
        self.reset_cursor();
    }

    pub fn hide_info_bar(&self) {
        self.send(UIEvent::HideInfoBar);
    }

    pub fn open_media(&self, path: PathBuf) {
        self.set_cursor_waiting();
        self.send(UIEvent::OpenMedia(path));
    }

    pub fn reset_cursor(&self) {
        self.send(UIEvent::ResetCursor);
    }

    pub fn restore_context(&self) {
        self.send(UIEvent::RestoreContext);
    }

    pub fn show_all(&self) {
        self.send(UIEvent::ShowAll);
    }

    pub fn seek(&self, target: Timestamp, flags: gst::SeekFlags) {
        self.send(UIEvent::Seek { target, flags });
    }

    pub fn set_cursor_waiting(&self) {
        self.send(UIEvent::SetCursorWaiting);
    }

    pub fn show_error<Msg>(&self, msg: Msg)
    where
        Msg: Into<Cow<'static, str>>,
    {
        self.send(UIEvent::ShowError(msg.into()));
    }

    pub fn show_info<Msg>(&self, msg: Msg)
    where
        Msg: Into<Cow<'static, str>>,
    {
        self.send(UIEvent::ShowInfo(msg.into()));
    }

    pub fn switch_to(&self, ctx: UIFocusContext) {
        self.send(UIEvent::SwitchTo(ctx));
    }

    // Call `restore_context` to retrieve initial state
    pub fn temporarily_switch_to(&self, ctx: UIFocusContext) {
        self.send(UIEvent::TemporarilySwitchTo(ctx));
    }

    pub fn update_focus(&self) {
        self.send(UIEvent::UpdateFocus);
    }
}

pub struct UIEventHandler {
    receiver: async_mpsc::Receiver<UIEvent>,
    app: gtk::Application,
    window: gtk::ApplicationWindow,
    main_ctrl: Option<Rc<RefCell<MainController>>>,
    info_bar_ctrl: InfoBarController,
    saved_context: Option<UIFocusContext>,
    focus: UIFocusContext,
}

impl UIEventHandler {
    pub fn new_pair(app: &gtk::Application, builder: &gtk::Builder) -> (Self, UIEventSender) {
        let (sender, receiver) = async_mpsc::channel(UI_EVENT_CHANNEL_CAPACITY);
        let ui_event_sender = UIEventSender(RefCell::new(sender));

        let handler = UIEventHandler {
            receiver,
            app: app.clone(),
            window: builder.get_object("application-window").unwrap(),
            main_ctrl: None,
            info_bar_ctrl: InfoBarController::new(app, builder, &ui_event_sender),
            saved_context: None,
            focus: UIFocusContext::PlaybackPage,
        };

        (handler, ui_event_sender)
    }

    pub fn have_main_ctrl(&mut self, main_ctrl: &Rc<RefCell<MainController>>) {
        self.main_ctrl = Some(Rc::clone(&main_ctrl));
        self.info_bar_ctrl.have_main_ctrl(main_ctrl);
    }

    pub fn spawn(mut self) {
        assert!(self.main_ctrl.is_some());
        spawn!(async move {
            while let Some(event) = self.receiver.next().await {
                debug!("handling event {:?}", event);
                if self.handle(event).is_err() {
                    break;
                }
            }
        });
    }

    #[inline]
    fn main_ctrl(&self) -> Ref<'_, MainController> {
        self.main_ctrl.as_ref().unwrap().borrow()
    }

    #[inline]
    fn main_ctrl_mut(&self) -> RefMut<'_, MainController> {
        self.main_ctrl.as_ref().unwrap().borrow_mut()
    }

    fn handle(&mut self, event: UIEvent) -> Result<(), ()> {
        match event {
            UIEvent::CancelSelectMedia => self.main_ctrl_mut().cancel_select_media(),
            UIEvent::HideInfoBar => self.info_bar_ctrl.hide(),
            UIEvent::OpenMedia(path) => self.main_ctrl_mut().open_media(path),
            UIEvent::ResetCursor => self.reset_cursor(),
            UIEvent::RestoreContext => self.restore_context(),
            UIEvent::ShowAll => self.show_all(),
            UIEvent::Seek { target, flags } => self.main_ctrl_mut().seek(target, flags),
            UIEvent::SetCursorWaiting => self.set_cursor_waiting(),
            UIEvent::ShowError(msg) => self.info_bar_ctrl.show_error(&msg),
            UIEvent::ShowInfo(msg) => self.info_bar_ctrl.show_info(&msg),
            UIEvent::SwitchTo(focus_ctx) => self.switch_to(focus_ctx),
            UIEvent::TemporarilySwitchTo(focus_ctx) => {
                self.save_context();
                self.bind_accels_for(focus_ctx);
            }
            UIEvent::UpdateFocus => self.update_focus(),
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
        let main_ctrl = self.main_ctrl();
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
