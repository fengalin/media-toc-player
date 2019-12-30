use gtk;
use gtk::prelude::*;

use crate::media::PlaybackPipeline;

use super::UIController;

pub struct PerspectiveController {
    pub(super) menu_btn: gtk::MenuButton,
    pub(super) popover: gtk::PopoverMenu,
    pub(super) stack: gtk::Stack,
}

impl PerspectiveController {
    pub fn new(builder: &gtk::Builder) -> Self {
        let mut ctrl = PerspectiveController {
            menu_btn: builder.get_object("perspective-menu-btn").unwrap(),
            popover: builder.get_object("perspective-popovermenu").unwrap(),
            stack: builder.get_object("perspective-stack").unwrap(),
        };

        ctrl.cleanup();

        ctrl
    }
}

impl UIController for PerspectiveController {
    fn new_media(&mut self, _pipeline: &PlaybackPipeline) {
        self.menu_btn.set_sensitive(true);
    }

    fn cleanup(&mut self) {
        self.menu_btn.set_sensitive(false);
    }
}
