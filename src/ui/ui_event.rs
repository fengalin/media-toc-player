use glib;
use gstreamer as gst;
use gtk;
use std::{borrow::Cow, path::PathBuf};

#[derive(Clone)]
pub enum UIEvent {
    CancelSelectMedia,
    OpenMedia(PathBuf),
    Seek {
        position: u64,
        flags: gst::SeekFlags,
    },
    ShowAll,
    SetCursorWaiting,
    ShowError(Cow<'static, str>),
    ShowInfo(Cow<'static, str>),
}

#[derive(Clone)]
pub struct UIEventSender(glib::Sender<UIEvent>);

#[allow(unused_must_use)]
impl UIEventSender {
    pub fn cancel_select_media(&self) {
        self.0.send(UIEvent::CancelSelectMedia);
    }

    pub fn open_media(&self, path: PathBuf) {
        // Trigger the message asynchronously otherwise the waiting cursor might not show up
        let mut path = Some(path);
        let sender = self.0.clone();
        gtk::idle_add(move || {
            if let Some(path) = path.take() {
                sender.send(UIEvent::OpenMedia(path));
            }
            glib::Continue(false)
        });
    }

    pub fn show_all(&self) {
        self.0.send(UIEvent::ShowAll);
    }

    pub fn seek(&self, position: u64, flags: gst::SeekFlags) {
        self.0.send(UIEvent::Seek { position, flags });
    }

    pub fn set_cursor_waiting(&self) {
        self.0.send(UIEvent::SetCursorWaiting);
    }

    pub fn show_error<Msg>(&self, msg: Msg)
    where
        Msg: Into<Cow<'static, str>>,
    {
        self.0.send(UIEvent::ShowError(msg.into()));
    }

    pub fn show_info<Msg>(&self, msg: Msg)
    where
        Msg: Into<Cow<'static, str>>,
    {
        self.0.send(UIEvent::ShowInfo(msg.into()));
    }
}

impl From<glib::Sender<UIEvent>> for UIEventSender {
    fn from(glib_ui_event: glib::Sender<UIEvent>) -> Self {
        UIEventSender(glib_ui_event)
    }
}
