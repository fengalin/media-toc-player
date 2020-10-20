use glib::clone;
use gtk::prelude::*;

use std::{cell::RefCell, rc::Rc};

use super::{MainController, StreamsController, UIDispatcher, UIEventSender, UIFocusContext};

pub struct StreamsDispatcher;
impl UIDispatcher for StreamsDispatcher {
    type Controller = StreamsController;

    fn setup(
        streams_ctrl: &mut StreamsController,
        _main_ctrl_rc: &Rc<RefCell<MainController>>,
        _app: &gtk::Application,
        ui_event: &UIEventSender,
    ) {
        streams_ctrl.video.treeview.connect_cursor_changed(
            clone!(@strong ui_event => move |_| ui_event.stream_clicked(gst::StreamType::VIDEO)),
        );

        streams_ctrl.audio.treeview.connect_cursor_changed(
            clone!(@strong ui_event => move |_| ui_event.stream_clicked(gst::StreamType::AUDIO)),
        );

        streams_ctrl.text.treeview.connect_cursor_changed(
            clone!(@strong ui_event => move |_| ui_event.stream_clicked(gst::StreamType::TEXT)),
        );

        streams_ctrl
            .page
            .connect_map(clone!(@strong ui_event => move |_| {
                ui_event.switch_to(UIFocusContext::StreamsPage);
            }));
    }
}
