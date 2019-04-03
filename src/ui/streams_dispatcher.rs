use gtk;
use gtk::prelude::*;

use std::{cell::RefCell, rc::Rc};

use super::{MainController, UIDispatcher};

macro_rules! on_stream_selected(
    ($main_ctrl_rc:expr, $store:ident, $selected:ident) => (
        {
            let main_ctrl_rc_cb = Rc::clone(&$main_ctrl_rc);
            move |_, tree_path, _| {
                let mut main_ctrl = main_ctrl_rc_cb.borrow_mut();
                let streams_ctrl = &mut main_ctrl.streams_ctrl;

                if let Some(iter) = streams_ctrl.$store.get_iter(tree_path) {
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
                        let main_ctrl_rc_idle = Rc::clone(&main_ctrl_rc_cb);
                        gtk::idle_add(move || {
                            main_ctrl_rc_idle.borrow_mut().select_streams(&streams);
                            glib::Continue(false)
                        });
                    }
                }
            }
        }
    );
);

pub struct StreamsDispatcher;
impl UIDispatcher for StreamsDispatcher {
    fn setup(_gtk_app: &gtk::Application, main_ctrl_rc: &Rc<RefCell<MainController>>) {
        let main_ctrl = main_ctrl_rc.borrow();
        let streams_ctrl = &main_ctrl.streams_ctrl;

        // Video stream selection
        streams_ctrl
            .video_treeview
            .connect_row_activated(on_stream_selected!(
                main_ctrl_rc,
                video_store,
                video_selected
            ));

        // Audio stream selection
        streams_ctrl
            .audio_treeview
            .connect_row_activated(on_stream_selected!(
                main_ctrl_rc,
                audio_store,
                audio_selected
            ));

        // Text stream selection
        streams_ctrl
            .text_treeview
            .connect_row_activated(on_stream_selected!(main_ctrl_rc, text_store, text_selected));
    }
}
