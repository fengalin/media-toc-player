extern crate gtk;
use gtk::{BoxExt, WidgetExt};

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

    pub fn register_callbacks(&self, _: &Rc<RefCell<MainController>>) {}

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
