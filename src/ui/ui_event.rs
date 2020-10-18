use futures::channel::mpsc as async_mpsc;

use gstreamer as gst;

use std::{borrow::Cow, cell::RefCell, path::PathBuf};

use crate::media::Timestamp;

#[derive(Clone, Copy, Debug)]
pub enum UIFocusContext {
    InfoBar,
    PlaybackPage,
    StreamsPage,
}

#[derive(Debug)]
pub enum UIEvent {
    CancelSelectMedia,
    Eos,
    HideInfoBar,
    OpenMedia(PathBuf),
    PlayPause,
    Quit,
    ResetCursor,
    RestoreContext,
    Seek {
        target: Timestamp,
        flags: gst::SeekFlags,
    },
    SelectMedia,
    ShowAll,
    SetCursorWaiting,
    ShowError(Cow<'static, str>),
    ShowInfo(Cow<'static, str>),
    SwitchTo(UIFocusContext),
    TemporarilySwitchTo(UIFocusContext),
    UpdateFocus,
}

#[derive(Clone)]
pub struct UIEventSender(RefCell<async_mpsc::UnboundedSender<UIEvent>>);

#[allow(unused_must_use)]
impl UIEventSender {
    fn send(&self, event: UIEvent) {
        let _ = self.0.borrow_mut().unbounded_send(event);
    }

    pub fn cancel_select_media(&self) {
        self.send(UIEvent::CancelSelectMedia);
        self.reset_cursor();
    }

    pub fn eos(&self) {
        self.send(UIEvent::Eos);
    }

    pub fn hide_info_bar(&self) {
        self.send(UIEvent::HideInfoBar);
    }

    pub fn open_media(&self, path: PathBuf) {
        self.set_cursor_waiting();
        self.send(UIEvent::OpenMedia(path));
    }

    pub fn play_pause(&self) {
        self.send(UIEvent::PlayPause);
    }

    pub fn quit(&self) {
        self.send(UIEvent::Quit);
    }

    pub fn reset_cursor(&self) {
        self.send(UIEvent::ResetCursor);
    }

    pub fn restore_context(&self) {
        self.send(UIEvent::RestoreContext);
    }

    pub fn select_media(&self) {
        self.send(UIEvent::SelectMedia);
    }

    pub fn seek(&self, target: Timestamp, flags: gst::SeekFlags) {
        self.send(UIEvent::Seek { target, flags });
    }

    pub fn set_cursor_waiting(&self) {
        self.send(UIEvent::SetCursorWaiting);
    }

    pub fn show_all(&self) {
        self.send(UIEvent::ShowAll);
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

pub fn new_pair() -> (UIEventSender, async_mpsc::UnboundedReceiver<UIEvent>) {
    let (sender, receiver) = async_mpsc::unbounded();
    let sender = UIEventSender(RefCell::new(sender));

    (sender, receiver)
}
