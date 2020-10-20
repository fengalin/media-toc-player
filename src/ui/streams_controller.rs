use gettextrs::gettext;

use gtk::prelude::*;

use std::sync::Arc;

use crate::{media::PlaybackPipeline, metadata};

use super::{spawn, UIController};

const ALIGN_LEFT: f32 = 0f32;
const ALIGN_CENTER: f32 = 0.5f32;
const ALIGN_RIGHT: f32 = 1f32;

const STREAM_ID_COL: u32 = 0;
const STREAM_ID_DISPLAY_COL: u32 = 1;

const LANGUAGE_COL: u32 = 2;
const CODEC_COL: u32 = 3;
const COMMENT_COL: u32 = 4;

pub enum StreamClickedStatus {
    Changed,
    Unchanged,
}

pub(super) trait UIStreamImpl {
    const TYPE: gst::StreamType;

    fn new_media(store: &gtk::ListStore, iter: &gtk::TreeIter, caps_struct: &gst::StructureRef);
    fn init_treeview(treeview: &gtk::TreeView, store: &gtk::ListStore);

    fn add_text_column(
        treeview: &gtk::TreeView,
        title: &str,
        alignment: f32,
        col_id: u32,
        width: Option<i32>,
    ) {
        let col = gtk::TreeViewColumn::new();
        col.set_title(title);

        let renderer = gtk::CellRendererText::new();
        renderer.set_alignment(alignment, ALIGN_CENTER);
        col.pack_start(&renderer, true);
        col.add_attribute(&renderer, "text", col_id as i32);

        if let Some(width) = width {
            renderer.set_fixed_size(width, -1);
        }

        treeview.append_column(&col);
    }
}

pub(super) struct UIStream<Impl: UIStreamImpl> {
    pub(super) treeview: gtk::TreeView,
    store: gtk::ListStore,
    selected: Option<Arc<str>>,
    phantom: std::marker::PhantomData<Impl>,
}

impl<Impl: UIStreamImpl> UIStream<Impl> {
    fn new(treeview: gtk::TreeView, store: gtk::ListStore) -> Self {
        UIStream {
            treeview,
            store,
            selected: None,
            phantom: std::marker::PhantomData,
        }
    }

    fn init_treeview(&self) {
        Impl::init_treeview(&self.treeview, &self.store);
    }

    fn cleanup(&mut self) {
        self.selected = None;
        self.treeview
            .set_cursor(&gtk::TreePath::new(), None::<&gtk::TreeViewColumn>, false);
        self.store.clear();
    }

    fn new_media(&mut self, streams: &metadata::Streams) {
        let sorted_collection = streams.collection(Impl::TYPE).sorted();
        for stream in sorted_collection {
            let iter = self.add_stream(stream);
            let caps_structure = stream.caps.get_structure(0).unwrap();
            Impl::new_media(&self.store, &iter, &caps_structure);
        }

        self.selected = self.store.get_iter_first().map(|ref iter| {
            self.treeview.get_selection().select_iter(iter);
            self.store
                .get_value(iter, STREAM_ID_COL as i32)
                .get::<String>()
                .unwrap()
                .unwrap()
                .into()
        });
    }

    fn add_stream(&self, stream: &metadata::Stream) -> gtk::TreeIter {
        let id_parts: Vec<&str> = stream.id.split('/').collect();
        let stream_id_display = if id_parts.len() == 2 {
            id_parts[1].to_owned()
        } else {
            gettext("unknown")
        };

        let iter = self.store.insert_with_values(
            None,
            &[STREAM_ID_COL, STREAM_ID_DISPLAY_COL],
            &[&stream.id.as_ref(), &stream_id_display],
        );

        let lang = stream
            .tags
            .get_index::<gst::tags::LanguageName>(0)
            .or_else(|| stream.tags.get_index::<gst::tags::LanguageCode>(0))
            .and_then(|value| value.get())
            .unwrap_or("-");
        self.store
            .set_value(&iter, LANGUAGE_COL, &glib::Value::from(lang));

        if let Some(comment) = stream
            .tags
            .get_index::<gst::tags::Comment>(0)
            .and_then(|value| value.get())
        {
            self.store
                .set_value(&iter, COMMENT_COL, &glib::Value::from(comment));
        }

        self.store.set_value(
            &iter,
            CODEC_COL,
            &glib::Value::from(&stream.codec_printable),
        );

        iter
    }

    fn stream_clicked(&mut self) -> StreamClickedStatus {
        if let (Some(cursor_path), _) = self.treeview.get_cursor() {
            if let Some(iter) = self.store.get_iter(&cursor_path) {
                let stream = self
                    .store
                    .get_value(&iter, STREAM_ID_COL as i32)
                    .get::<String>()
                    .unwrap()
                    .unwrap()
                    .into();
                let stream_to_select = match &self.selected {
                    Some(stream_id) => {
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
                    self.selected = Some(new_stream);
                    return StreamClickedStatus::Changed;
                }
            }
        }

        StreamClickedStatus::Unchanged
    }
}

pub(super) struct UIStreamVideoImpl;
impl UIStreamVideoImpl {
    const VIDEO_WIDTH_COL: u32 = 5;
    const VIDEO_HEIGHT_COL: u32 = 6;
}

impl UIStreamImpl for UIStreamVideoImpl {
    const TYPE: gst::StreamType = gst::StreamType::VIDEO;

    fn new_media(store: &gtk::ListStore, iter: &gtk::TreeIter, caps_struct: &gst::StructureRef) {
        if let Ok(Some(width)) = caps_struct.get::<i32>("width") {
            store.set_value(iter, Self::VIDEO_WIDTH_COL, &glib::Value::from(&width));
        }
        if let Ok(Some(height)) = caps_struct.get::<i32>("height") {
            store.set_value(iter, Self::VIDEO_HEIGHT_COL, &glib::Value::from(&height));
        }
    }

    fn init_treeview(treeview: &gtk::TreeView, store: &gtk::ListStore) {
        treeview.set_model(Some(store));

        // Video
        Self::add_text_column(
            treeview,
            &gettext("Stream id"),
            ALIGN_LEFT,
            STREAM_ID_DISPLAY_COL,
            Some(200),
        );
        Self::add_text_column(
            treeview,
            &gettext("Language"),
            ALIGN_CENTER,
            LANGUAGE_COL,
            None,
        );
        Self::add_text_column(treeview, &gettext("Codec"), ALIGN_LEFT, CODEC_COL, None);
        Self::add_text_column(
            treeview,
            &gettext("Width"),
            ALIGN_RIGHT,
            Self::VIDEO_WIDTH_COL,
            None,
        );
        Self::add_text_column(
            treeview,
            &gettext("Height"),
            ALIGN_RIGHT,
            Self::VIDEO_HEIGHT_COL,
            None,
        );
        Self::add_text_column(treeview, &gettext("Comment"), ALIGN_LEFT, COMMENT_COL, None);
    }
}

pub(super) struct UIStreamAudioImpl;
impl UIStreamAudioImpl {
    const AUDIO_RATE_COL: u32 = 5;
    const AUDIO_CHANNELS_COL: u32 = 6;
}

impl UIStreamImpl for UIStreamAudioImpl {
    const TYPE: gst::StreamType = gst::StreamType::AUDIO;

    fn new_media(store: &gtk::ListStore, iter: &gtk::TreeIter, caps_struct: &gst::StructureRef) {
        if let Ok(Some(rate)) = caps_struct.get::<i32>("rate") {
            store.set_value(&iter, Self::AUDIO_RATE_COL, &glib::Value::from(&rate));
        }
        if let Ok(Some(channels)) = caps_struct.get::<i32>("channels") {
            store.set_value(
                &iter,
                Self::AUDIO_CHANNELS_COL,
                &glib::Value::from(&channels),
            );
        }
    }

    fn init_treeview(treeview: &gtk::TreeView, store: &gtk::ListStore) {
        treeview.set_model(Some(store));

        Self::add_text_column(
            treeview,
            &gettext("Stream id"),
            ALIGN_LEFT,
            STREAM_ID_DISPLAY_COL,
            Some(200),
        );
        Self::add_text_column(
            treeview,
            &gettext("Language"),
            ALIGN_CENTER,
            LANGUAGE_COL,
            None,
        );
        Self::add_text_column(treeview, &gettext("Codec"), ALIGN_LEFT, CODEC_COL, None);
        Self::add_text_column(
            treeview,
            &gettext("Rate"),
            ALIGN_RIGHT,
            Self::AUDIO_RATE_COL,
            None,
        );
        Self::add_text_column(
            treeview,
            &gettext("Channels"),
            ALIGN_RIGHT,
            Self::AUDIO_CHANNELS_COL,
            None,
        );
        Self::add_text_column(treeview, &gettext("Comment"), ALIGN_LEFT, COMMENT_COL, None);
    }
}

pub(super) struct UIStreamTextImpl;
impl UIStreamTextImpl {
    const TEXT_FORMAT_COL: u32 = 5;
}

impl UIStreamImpl for UIStreamTextImpl {
    const TYPE: gst::StreamType = gst::StreamType::TEXT;

    fn new_media(store: &gtk::ListStore, iter: &gtk::TreeIter, caps_struct: &gst::StructureRef) {
        if let Ok(Some(format)) = caps_struct.get::<&str>("format") {
            store.set_value(&iter, Self::TEXT_FORMAT_COL, &glib::Value::from(&format));
        }
    }

    fn init_treeview(treeview: &gtk::TreeView, store: &gtk::ListStore) {
        treeview.set_model(Some(store));

        Self::add_text_column(
            treeview,
            &gettext("Stream id"),
            ALIGN_LEFT,
            STREAM_ID_DISPLAY_COL,
            Some(200),
        );
        Self::add_text_column(
            treeview,
            &gettext("Language"),
            ALIGN_CENTER,
            LANGUAGE_COL,
            None,
        );
        Self::add_text_column(treeview, &gettext("Codec"), ALIGN_LEFT, CODEC_COL, None);
        Self::add_text_column(
            treeview,
            &gettext("Format"),
            ALIGN_LEFT,
            Self::TEXT_FORMAT_COL,
            None,
        );
        Self::add_text_column(treeview, &gettext("Comment"), ALIGN_LEFT, COMMENT_COL, None);
    }
}

pub struct StreamsController {
    pub(super) page: gtk::Grid,

    pub(super) video: UIStream<UIStreamVideoImpl>,
    pub(super) audio: UIStream<UIStreamAudioImpl>,
    pub(super) text: UIStream<UIStreamTextImpl>,
}

impl UIController for StreamsController {
    fn new_media(&mut self, pipeline: &PlaybackPipeline) {
        self.video.new_media(&pipeline.info.streams);
        self.audio.new_media(&pipeline.info.streams);
        self.text.new_media(&pipeline.info.streams);
    }

    fn cleanup(&mut self) {
        self.video.cleanup();
        self.audio.cleanup();
        self.text.cleanup();
    }

    fn grab_focus(&self) {
        // grab focus asynchronoulsy because it triggers the `cursor_changed` signal
        // which needs to check if the stream has changed
        let audio_treeview = self.audio.treeview.clone();
        spawn(async move {
            audio_treeview.grab_focus();
        });
    }
}

impl StreamsController {
    pub fn new(builder: &gtk::Builder) -> Self {
        let mut ctrl = StreamsController {
            page: builder.get_object("streams-grid").unwrap(),

            video: UIStream::new(
                builder.get_object("video_streams-treeview").unwrap(),
                builder.get_object("video_streams-liststore").unwrap(),
            ),

            audio: UIStream::new(
                builder.get_object("audio_streams-treeview").unwrap(),
                builder.get_object("audio_streams-liststore").unwrap(),
            ),

            text: UIStream::new(
                builder.get_object("text_streams-treeview").unwrap(),
                builder.get_object("text_streams-liststore").unwrap(),
            ),
        };

        ctrl.cleanup();

        ctrl.video.init_treeview();
        ctrl.audio.init_treeview();
        ctrl.text.init_treeview();

        ctrl
    }

    pub(super) fn stream_clicked(&mut self, type_: gst::StreamType) -> StreamClickedStatus {
        match type_ {
            gst::StreamType::VIDEO => self.video.stream_clicked(),
            gst::StreamType::AUDIO => self.audio.stream_clicked(),
            gst::StreamType::TEXT => self.text.stream_clicked(),
            other => unimplemented!("{:?}", other),
        }
    }

    pub fn selected_streams(&self) -> Vec<Arc<str>> {
        let mut streams: Vec<Arc<str>> = Vec::new();
        if let Some(stream) = self.video.selected.as_ref() {
            streams.push(Arc::clone(stream));
        }
        if let Some(stream) = self.audio.selected.as_ref() {
            streams.push(Arc::clone(stream));
        }
        if let Some(stream) = self.text.selected.as_ref() {
            streams.push(Arc::clone(stream));
        }

        streams
    }
}
