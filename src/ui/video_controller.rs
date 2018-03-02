use gtk;

use glib::ObjectExt;
use glib::signal::SignalHandlerId;
use gtk::{BoxExt, Inhibit, WidgetExt};

use std::rc::Rc;
use std::cell::RefCell;

use media::Context;

use super::MainController;

pub struct VideoController {
    container: gtk::Box,
    video_widget: gtk::Widget,
    cleaner_id: Option<SignalHandlerId>,
}

impl VideoController {
    pub fn new(builder: &gtk::Builder) -> Self {
        let container: gtk::Box = builder.get_object("video-container").unwrap();
        let video_widget = Context::get_video_widget();
        container.pack_start(&video_widget, true, true, 0);
        container.reorder_child(&video_widget, 0);

        VideoController {
            container: container,
            video_widget: video_widget,
            cleaner_id: None,
        }
    }

    pub fn register_callbacks(&self, main_ctrl: &Rc<RefCell<MainController>>) {
        let main_ctrl_clone = Rc::clone(main_ctrl);
        self.container
            .connect_button_press_event(move |_, _event_button| {
                main_ctrl_clone.borrow_mut().play_pause();
                Inhibit(false)
            });
    }

    pub fn cleanup(&mut self) {
        if self.cleaner_id.is_none() {
            self.cleaner_id = Some(self.video_widget.connect_draw(|widget, cr| {
                let allocation = widget.get_allocation();
                cr.set_source_rgb(0f64, 0f64, 0f64);
                cr.rectangle(
                    0f64,
                    0f64,
                    f64::from(allocation.width),
                    f64::from(allocation.height),
                );
                cr.fill();

                Inhibit(true)
            }));
            self.video_widget.queue_draw();
        }
    }

    pub fn new_media(&mut self, context: &Context) {
        if let Some(cleaner_id) = self.cleaner_id.take() {
            self.video_widget.disconnect(cleaner_id);
        }

        let has_video = context
            .info
            .lock()
            .expect("Failed to lock media info while initializing video controller")
            .streams
            .video_selected
            .is_some();

        if has_video {
            self.video_widget.show();
        } else {
            self.video_widget.hide();
        }
    }
}
