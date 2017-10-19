extern crate gtk;
use gtk::BoxExt;

use std::rc::Rc;
use std::cell::RefCell;

use media::Context;

use super::MainController;

pub struct VideoController {
}

impl VideoController {
    pub fn new(builder: &gtk::Builder) -> Self {
        let container: gtk::Box = builder.get_object("video-container").unwrap();
        let video_widget = Context::get_video_widget();
        container.pack_start(&video_widget, true, true, 0);
        container.reorder_child(&video_widget, 0);

        VideoController {
        }
    }

    pub fn register_callbacks(&self, _: &Rc<RefCell<MainController>>) {
    }

    pub fn new_media(&mut self, _context: &Context) {
    }
}
