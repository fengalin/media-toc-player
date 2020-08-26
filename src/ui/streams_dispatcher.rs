use gtk::prelude::*;

use std::{cell::RefCell, rc::Rc};

use crate::spawn;

use super::{MainController, StreamsController, UIDispatcher, UIEventSender, UIFocusContext};

macro_rules! on_stream_selected(
    ($main_ctrl_rc:expr, $store:ident, $selected:ident) => (
        {
            let main_ctrl_rc_cb = Rc::clone(&$main_ctrl_rc);
            move |treeview| {
                if let (Some(cursor_path), _) = treeview.get_cursor() {
                    let mut main_ctrl = main_ctrl_rc_cb.borrow_mut();
                    let streams_ctrl = &mut main_ctrl.streams_ctrl;

                    if let Some(iter) = streams_ctrl.$store.get_iter(&cursor_path) {
                        let stream = streams_ctrl.get_stream_at(&streams_ctrl.$store, &iter);
                        let stream_to_select = match streams_ctrl.$selected {
                            Some(ref stream_id) => {
                                if stream_id != &stream {
                                    // Stream has changed
                                    Some(stream)
                                } else {
                                    None
                                }
                            }
                            None => Some(stream),
                        };
                        if let Some(new_stream) = stream_to_select {
                            streams_ctrl.$selected = Some(new_stream);
                            let streams = streams_ctrl.get_selected_streams();

                            // Asynchronoulsy notify the main controller
                            let main_ctrl_rc = Rc::clone(&main_ctrl_rc_cb);
                            spawn!(async move {
                                main_ctrl_rc.borrow_mut().select_streams(&streams);
                            });
                        }
                    }
                }
            }
        }
    );
);

pub struct StreamsDispatcher;
impl UIDispatcher for StreamsDispatcher {
    type Controller = StreamsController;

    fn setup(
        streams_ctrl: &mut StreamsController,
        main_ctrl_rc: &Rc<RefCell<MainController>>,
        _app: &gtk::Application,
        ui_event: &UIEventSender,
    ) {
        // Video stream selection
        streams_ctrl
            .video_treeview
            .connect_cursor_changed(on_stream_selected!(
                main_ctrl_rc,
                video_store,
                video_selected
            ));

        // Audio stream selection
        streams_ctrl
            .audio_treeview
            .connect_cursor_changed(on_stream_selected!(
                main_ctrl_rc,
                audio_store,
                audio_selected
            ));

        // Text stream selection
        streams_ctrl
            .text_treeview
            .connect_cursor_changed(on_stream_selected!(main_ctrl_rc, text_store, text_selected));

        let ui_event = ui_event.clone();
        streams_ctrl.page.connect_map(move |_| {
            ui_event.switch_to(UIFocusContext::StreamsPage);
        });
    }
}
