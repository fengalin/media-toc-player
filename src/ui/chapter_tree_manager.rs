use bitflags::bitflags;

use gettextrs::gettext;
use gstreamer as gst;

use gtk::prelude::*;

use std::{borrow::Cow, fmt};

use crate::{
    media::Timestamp,
    metadata::{get_default_chapter_title, TocVisitor},
};

const START_COL: u32 = 0;
const END_COL: u32 = 1;
const TITLE_COL: u32 = 2;
const START_STR_COL: u32 = 3;
const END_STR_COL: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct ChapterTimestamps {
    pub start: Timestamp,
    pub end: Timestamp,
}

impl ChapterTimestamps {
    pub fn new_from_u64(start: u64, end: u64) -> Self {
        ChapterTimestamps {
            start: Timestamp::new(start),
            end: Timestamp::new(end),
        }
    }
}

impl fmt::Display for ChapterTimestamps {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "start {}, end {}",
            self.start.for_humans().to_string(),
            self.end.for_humans().to_string(),
        )
    }
}

pub struct ChapterIterStart {
    pub iter: gtk::TreeIter,
    pub start: Timestamp,
}

pub enum PositionStatus {
    ChapterChanged {
        prev_chapter: Option<ChapterIterStart>,
    },
    ChapterNotChanged,
}

impl From<Option<ChapterIterStart>> for PositionStatus {
    fn from(prev_chapter: Option<ChapterIterStart>) -> Self {
        PositionStatus::ChapterChanged { prev_chapter }
    }
}

bitflags! {
    struct ColumnOptions: u32 {
        const NONE = 0b0000_0000;
        const CAN_EXPAND = 0b0000_0001;
    }
}

#[derive(Clone)]
pub struct ChapterEntry<'entry> {
    store: &'entry gtk::TreeStore,
    iter: Cow<'entry, gtk::TreeIter>,
}

impl<'entry> ChapterEntry<'entry> {
    fn new(store: &'entry gtk::TreeStore, iter: &'entry gtk::TreeIter) -> ChapterEntry<'entry> {
        ChapterEntry {
            store,
            iter: Cow::Borrowed(iter),
        }
    }

    fn new_owned(store: &'entry gtk::TreeStore, iter: gtk::TreeIter) -> ChapterEntry<'entry> {
        ChapterEntry {
            store,
            iter: Cow::Owned(iter),
        }
    }

    pub fn iter(&self) -> &gtk::TreeIter {
        self.iter.as_ref()
    }

    pub fn start(&self) -> Timestamp {
        self.store
            .get_value(&self.iter, START_COL as i32)
            .get_some::<u64>()
            .unwrap()
            .into()
    }

    pub fn end(&self) -> Timestamp {
        self.store
            .get_value(&self.iter, END_COL as i32)
            .get_some::<u64>()
            .unwrap()
            .into()
    }

    pub fn timestamps(&self) -> ChapterTimestamps {
        ChapterTimestamps {
            start: self.start(),
            end: self.end(),
        }
    }
}

struct ChapterTree {
    store: gtk::TreeStore,
    iter: Option<gtk::TreeIter>,
    selected: Option<gtk::TreeIter>,
}

impl ChapterTree {
    fn new(store: gtk::TreeStore) -> Self {
        ChapterTree {
            store,
            iter: None,
            selected: None,
        }
    }

    fn store(&self) -> &gtk::TreeStore {
        &self.store
    }

    fn clear(&mut self) {
        self.selected = None;
        self.iter = None;
        self.store.clear();
    }

    fn unselect(&mut self) {
        self.selected = None;
    }

    fn rewind(&mut self) {
        self.iter = self.store.get_iter_first();
        self.selected = match &self.iter_chapter() {
            Some(first_chapter) => {
                if first_chapter.start() == Timestamp::default() {
                    self.iter.clone()
                } else {
                    None
                }
            }
            None => None,
        };
    }

    fn chapter_from_path(&self, tree_path: &gtk::TreePath) -> Option<ChapterEntry<'_>> {
        self.store
            .get_iter(tree_path)
            .map(|iter| ChapterEntry::new_owned(&self.store, iter))
    }

    fn selected_chapter(&self) -> Option<ChapterEntry<'_>> {
        self.selected
            .as_ref()
            .map(|selected| ChapterEntry::new(&self.store, selected))
    }

    fn selected_timestamps(&self) -> Option<ChapterTimestamps> {
        self.selected_chapter().map(|chapter| chapter.timestamps())
    }

    fn selected_path(&self) -> Option<gtk::TreePath> {
        self.selected
            .as_ref()
            .and_then(|sel_iter| self.store.get_path(sel_iter))
    }

    fn iter_chapter(&self) -> Option<ChapterEntry<'_>> {
        self.iter
            .as_ref()
            .map(|iter| ChapterEntry::new(&self.store, iter))
    }

    fn iter_timestamps(&self) -> Option<ChapterTimestamps> {
        self.iter_chapter().map(|chapter| chapter.timestamps())
    }

    fn new_iter(&self) -> Iter<'_> {
        Iter::new(&self.store)
    }

    fn next(&mut self) -> Option<ChapterEntry<'_>> {
        match self.iter.take() {
            Some(iter) => {
                if self.store.iter_next(&iter) {
                    self.iter = Some(iter);
                    let store = &self.store;
                    self.iter
                        .as_ref()
                        .map(|iter| ChapterEntry::new(store, iter))
                } else {
                    None
                }
            }
            None => None,
        }
    }

    fn pick_next(&self) -> Option<ChapterEntry<'_>> {
        match self.selected.as_ref() {
            Some(selected) => {
                let iter = selected.clone();
                if self.store.iter_next(&iter) {
                    Some(ChapterEntry::new_owned(&self.store, iter))
                } else {
                    // FIXME: with hierarchical tocs, this might be a case where
                    // we should check whether the parent node contains something
                    None
                }
            }
            None => self
                .store
                .get_iter_first()
                .map(|first_iter| ChapterEntry::new_owned(&self.store, first_iter)),
        }
    }

    fn previous(&mut self) -> Option<ChapterEntry<'_>> {
        match self.iter.take() {
            Some(iter) => {
                if self.store.iter_previous(&iter) {
                    self.iter = Some(iter);
                    let store = &self.store;
                    self.iter
                        .as_ref()
                        .map(|iter| ChapterEntry::new(store, iter))
                } else {
                    None
                }
            }
            None => None,
        }
    }

    fn pick_previous(&self) -> Option<ChapterEntry<'_>> {
        match self.selected.as_ref() {
            Some(selected) => {
                let prev_iter = selected.clone();
                if self.store.iter_previous(&prev_iter) {
                    Some(ChapterEntry::new_owned(&self.store, prev_iter))
                } else {
                    // FIXME: with hierarchical tocs, this might be a case where
                    // we should check whether the parent node contains something
                    None
                }
            }
            None => self.store.get_iter_first().map(|iter| {
                let mut last_iter = iter.clone();
                while self.store.iter_next(&iter) {
                    last_iter = iter.clone();
                }
                ChapterEntry::new_owned(&self.store, last_iter)
            }),
        }
    }

    fn add_unchecked(&self, ts: ChapterTimestamps, title: &str) -> gtk::TreeIter {
        self.store.insert_with_values(
            None,
            None,
            &[START_COL, END_COL, TITLE_COL, START_STR_COL, END_STR_COL],
            &[
                &ts.start.as_u64(),
                &ts.end.as_u64(),
                &title,
                &ts.start.for_humans().to_string(),
                &ts.end.for_humans().to_string(),
            ],
        )
    }

    fn select_by_ts(&mut self, ts: Timestamp) -> PositionStatus {
        let prev_sel_chapter = match self.selected_timestamps() {
            Some(sel_ts) => {
                if ts >= sel_ts.start && ts < sel_ts.end {
                    // regular case: current timestamp in current chapter => don't change anything
                    // this check is here to save time in the most frequent case
                    return PositionStatus::ChapterNotChanged;
                }

                assert!(self.selected.is_some());
                Some(ChapterIterStart {
                    iter: self.selected.take().unwrap(),
                    start: sel_ts.start,
                })
            }
            None => None,
        };

        if self.iter.is_some() {
            // not in selected_iter or selected_iter not defined yet
            // => search for a chapter matching current ts
            let mut searching_forward = true;
            loop {
                let iter_ts = self.iter_timestamps().expect("couldn't get start & end");
                if ts >= iter_ts.start && ts < iter_ts.end {
                    // current timestamp is in current chapter
                    self.selected = self.iter.clone();
                    // ChapterChanged
                    return prev_sel_chapter.into();
                } else if ts >= iter_ts.end && searching_forward {
                    // current timestamp is after iter and we were already searching forward
                    let cur_iter = self.iter.clone();
                    self.next();
                    if self.iter.is_none() {
                        // No more chapter => keep track of last iter:
                        // in case of a seek back, we'll start from here
                        self.iter = cur_iter;
                        break;
                    }
                } else if ts < iter_ts.start {
                    // current timestamp before iter
                    searching_forward = false;
                    self.previous();
                    if self.iter.is_none() {
                        // before first chapter
                        self.iter = self.store.get_iter_first();
                        // ChapterChanged
                        return prev_sel_chapter.into();
                    }
                } else {
                    // in a gap between two chapters
                    break;
                }
            }
        }

        // Couldn't find a chapter to select
        // consider that the chapter changed only if a chapter was selected before
        match prev_sel_chapter {
            Some(prev_sel_chapter) => Some(prev_sel_chapter).into(),
            None => PositionStatus::ChapterNotChanged,
        }
    }
}

pub struct ChapterTreeManager {
    tree: ChapterTree,
}

impl ChapterTreeManager {
    pub fn new(store: gtk::TreeStore) -> Self {
        ChapterTreeManager {
            tree: ChapterTree::new(store),
        }
    }

    pub fn init_treeview(&mut self, treeview: &gtk::TreeView) {
        treeview.set_model(Some(self.tree.store()));
        self.add_column(
            treeview,
            &gettext("Title"),
            TITLE_COL,
            ColumnOptions::CAN_EXPAND,
        );
        self.add_column(
            treeview,
            &gettext("Start"),
            START_STR_COL,
            ColumnOptions::NONE,
        );
        self.add_column(treeview, &gettext("End"), END_STR_COL, ColumnOptions::NONE);
    }

    fn add_column(
        &self,
        treeview: &gtk::TreeView,
        title: &str,
        col_id: u32,
        options: ColumnOptions,
    ) -> gtk::CellRendererText {
        let col = gtk::TreeViewColumn::new();
        col.set_title(title);

        let renderer = gtk::CellRendererText::new();

        col.pack_start(&renderer, true);
        col.add_attribute(&renderer, "text", col_id as i32);
        if options.contains(ColumnOptions::CAN_EXPAND) {
            col.set_min_width(70);
            col.set_expand(true);
        } else {
            // align right
            renderer.set_property_xalign(1f32);
        }
        treeview.append_column(&col);

        renderer
    }

    pub fn selected(&self) -> Option<ChapterEntry<'_>> {
        self.tree.selected_chapter()
    }

    pub fn selected_path(&self) -> Option<gtk::TreePath> {
        self.tree.selected_path()
    }

    pub fn chapter_from_path(&self, tree_path: &gtk::TreePath) -> Option<ChapterEntry<'_>> {
        self.tree.chapter_from_path(tree_path)
    }

    pub fn unselect(&mut self) {
        self.tree.unselect();
    }

    pub fn clear(&mut self) {
        self.tree.clear();
    }

    pub fn replace_with(&mut self, toc: &Option<gst::Toc>) {
        self.clear();

        if let Some(ref toc) = *toc {
            let mut toc_visitor = TocVisitor::new(toc);
            if !toc_visitor.enter_chapters() {
                return;
            }

            // FIXME: handle hierarchical Tocs
            while let Some(chapter) = toc_visitor.next_chapter() {
                assert_eq!(gst::TocEntryType::Chapter, chapter.get_entry_type());

                if let Some((start, end)) = chapter.get_start_stop_times() {
                    let ts = ChapterTimestamps::new_from_u64(start as u64, end as u64);

                    let title = chapter
                        .get_tags()
                        .and_then(|tags| {
                            tags.get::<gst::tags::Title>()
                                .and_then(|tag| tag.get().map(ToString::to_string))
                        })
                        .unwrap_or_else(get_default_chapter_title);

                    self.tree.add_unchecked(ts, &title);
                }
            }
        }

        self.tree.rewind();
    }

    pub fn iter(&self) -> Iter<'_> {
        self.tree.new_iter()
    }

    // Update chapter according to the given ts
    pub fn update_ts(&mut self, ts: Timestamp) -> PositionStatus {
        self.tree.select_by_ts(ts)
    }

    pub fn pick_next(&self) -> Option<ChapterEntry<'_>> {
        self.tree.pick_next()
    }

    pub fn pick_previous(&self) -> Option<ChapterEntry<'_>> {
        self.tree.pick_previous()
    }
}

pub struct Iter<'store> {
    store: &'store gtk::TreeStore,
    iter: Option<gtk::TreeIter>,
    is_first: bool,
}

impl<'store> Iter<'store> {
    fn new(store: &'store gtk::TreeStore) -> Self {
        Iter {
            store,
            iter: None,
            is_first: true,
        }
    }
}

impl<'store> Iterator for Iter<'store> {
    type Item = ChapterEntry<'store>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.is_first {
            if let Some(iter) = self.iter.as_mut() {
                if !self.store.iter_next(iter) {
                    self.iter = None;
                }
            }
        } else {
            self.iter = self.store.get_iter_first();
            self.is_first = false;
        }

        self.iter
            .clone()
            .map(|iter| ChapterEntry::new_owned(&self.store, iter))
    }
}
