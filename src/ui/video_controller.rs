extern crate gtk;
use gtk::{BoxExt, Inhibit, WidgetExt};

use std::rc::Rc;
use std::cell::RefCell;

use media::Context;

use super::MainController;

pub struct VideoController {
    video_widget: gtk::Widget,
}

impl VideoController {
    pub fn new(builder: &gtk::Builder) -> Self {
        let container: gtk::Box = builder.get_object("video-container").unwrap();
        let video_widget = Context::get_video_widget();
        container.pack_start(&video_widget, true, true, 0);
        container.reorder_child(&video_widget, 0);

        VideoController {
            video_widget: video_widget,
        }
    }

    pub fn register_callbacks(&self, main_ctrl: &Rc<RefCell<MainController>>) {
        let main_ctrl_clone = Rc::clone(main_ctrl);
        self.video_widget.connect_button_press_event(move |_, _event_button| {
            main_ctrl_clone.borrow_mut().play_pause();
            Inhibit(false)
        });
    }

    pub fn new_media(&mut self, context: &Context) {
        let has_video = context
            .info
            .lock()
            .expect("Failed to lock media info while initializing video controller")
            .video_best
            .is_some();

        if has_video {
            self.video_widget.show();
        } else {
            self.video_widget.hide();
        }
    }
}
