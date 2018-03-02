use gstreamer as gst;

use gtk;
use gtk::prelude::*;

use metadata::{Timestamp, TocVisit, TocVisitor};

const START_COL: u32 = 0;
const END_COL: u32 = 1;
const TITLE_COL: u32 = 2;
const START_STR_COL: u32 = 3;
const END_STR_COL: u32 = 4;

lazy_static! {
    static ref DEFAULT_TITLE: String = "untitled".to_owned();
}

pub struct ChapterEntry<'a> {
    store: &'a gtk::TreeStore,
    iter: &'a gtk::TreeIter,
}

impl<'a> ChapterEntry<'a> {
    pub fn new(store: &'a gtk::TreeStore, iter: &'a gtk::TreeIter) -> ChapterEntry<'a> {
        ChapterEntry {
            store: store,
            iter: iter,
        }
    }

    pub fn start(&self) -> u64 {
        ChapterEntry::get_start(self.store, self.iter)
    }

    pub fn get_start(store: &gtk::TreeStore, iter: &gtk::TreeIter) -> u64 {
        store
            .get_value(iter, START_COL as i32)
            .get::<u64>()
            .unwrap()
    }

    pub fn get_end(store: &gtk::TreeStore, iter: &gtk::TreeIter) -> u64 {
        store.get_value(iter, END_COL as i32).get::<u64>().unwrap()
    }
}

pub struct ChapterTreeManager {
    store: gtk::TreeStore,
    iter: Option<gtk::TreeIter>,
    selected_iter: Option<gtk::TreeIter>,
}

impl ChapterTreeManager {
    pub fn new_from(store: gtk::TreeStore) -> Self {
        ChapterTreeManager {
            store: store,
            iter: None,
            selected_iter: None,
        }
    }

    pub fn init_treeview(&self, treeview: &gtk::TreeView) {
        treeview.set_model(Some(&self.store));
        self.add_column(treeview, "Title", TITLE_COL, true, true);
        self.add_column(treeview, "Start", START_STR_COL, false, false);
        self.add_column(treeview, "End", END_STR_COL, false, false);
    }

    fn add_column(
        &self,
        treeview: &gtk::TreeView,
        title: &str,
        col_id: u32,
        can_expand: bool,
        is_editable: bool,
    ) {
        let col = gtk::TreeViewColumn::new();
        col.set_title(title);

        let renderer = gtk::CellRendererText::new();
        if is_editable {
            renderer.set_property_editable(true);
            let store_clone = self.store.clone();
            renderer.connect_edited(move |_, tree_path, value| {
                if let Some(iter) = store_clone.get_iter(&tree_path) {
                    store_clone.set_value(&iter, TITLE_COL, &gtk::Value::from(value));
                }
            });
        }

        col.pack_start(&renderer, true);
        col.add_attribute(&renderer, "text", col_id as i32);
        if can_expand {
            col.set_min_width(70);
            col.set_expand(can_expand);
        } else {
            // align right
            renderer.set_property_xalign(1f32);
        }
        treeview.append_column(&col);
    }

    pub fn get_selected_iter(&self) -> Option<gtk::TreeIter> {
        self.selected_iter.clone()
    }

    pub fn get_chapter_at_iter<'a: 'b, 'b>(&'a self, iter: &'a gtk::TreeIter) -> ChapterEntry<'b> {
        ChapterEntry::new(&self.store, iter)
    }

    pub fn get_iter(&self, tree_path: &gtk::TreePath) -> Option<gtk::TreeIter> {
        self.store.get_iter(tree_path)
    }

    pub fn unselect(&mut self) {
        self.selected_iter = None;
    }

    pub fn rewind(&mut self) {
        self.iter = self.store.get_iter_first();
    }

    pub fn clear(&mut self) {
        self.store.clear();
    }

    pub fn replace_with(&mut self, toc: &Option<gst::Toc>) {
        self.clear();

        if let &Some(ref toc) = toc {
            let mut toc_visitor = TocVisitor::new(toc);
            if !toc_visitor.enter_chapters() {
                return;
            }

            // FIXME: handle hierarchical Tocs
            while let Some(toc_visit) = toc_visitor.next() {
                match toc_visit {
                    TocVisit::Node(chapter) => {
                        assert_eq!(gst::TocEntryType::Chapter, chapter.get_entry_type());

                        if let Some((start, end)) = chapter.get_start_stop_times() {
                            let start = start as u64;
                            let end = end as u64;

                            let title = chapter.get_tags().map_or(None, |tags| {
                                tags.get::<gst::tags::Title>().map(|tag| {
                                    tag.get().unwrap().to_owned()
                                })
                            }).unwrap_or(DEFAULT_TITLE.to_owned());
                            self.store.insert_with_values(
                                None,
                                None,
                                &[START_COL, END_COL, TITLE_COL, START_STR_COL, END_STR_COL],
                                &[
                                    &start,
                                    &end,
                                    &title,
                                    &format!("{}", &Timestamp::format(start, false)),
                                    &format!("{}", &Timestamp::format(end, false)),
                                ],
                            );
                        }
                    }
                    _ => (),
                }
            }
        }

        self.iter = self.store.get_iter_first();

        self.selected_iter = match self.iter {
            Some(ref iter) => {
                if ChapterEntry::get_start(&self.store, iter) == 0 {
                    Some(iter.clone())
                } else {
                    None
                }
            }
            None => None,
        };
    }

    // Iterate over the chapters and apply func to all elements until the last
    // or until func returns false
    //
    // If first_iter is_some, iterate from first_iter
    pub fn for_each<F>(&self, first_iter: Option<gtk::TreeIter>, mut func: F)
    where
        F: FnMut(ChapterEntry) -> bool, // closure must return true to keep going
    {
        let iter = match first_iter {
            Some(first_iter) => first_iter,
            None => match self.store.get_iter_first() {
                Some(first_iter) => first_iter,
                None => return,
            },
        };

        while func(ChapterEntry::new(&self.store, &iter)) && self.store.iter_next(&iter) {}
    }

    // Update chapter according to the given position
    // Returns (has_changed, prev_selected_iter)
    pub fn update_position(&mut self, position: u64) -> (bool, Option<gtk::TreeIter>) {
        let has_changed = match self.selected_iter {
            Some(ref selected_iter) => {
                if position >= ChapterEntry::get_start(&self.store, selected_iter)
                    && position < ChapterEntry::get_end(&self.store, selected_iter)
                {
                    // regular case: position in current chapter => don't change anything
                    // this check is here to save time in the most frequent case
                    return (false, None);
                }
                // chapter has changed
                true
            }
            None => false,
        };

        let prev_selected_iter = self.selected_iter.take();

        if let Some(iter) = self.iter.take() {
            // not in selected_iter or selected_iter not defined yet => find current chapter
            let mut searching_forward = true;
            while if position >= ChapterEntry::get_start(&self.store, &iter)
                && position < ChapterEntry::get_end(&self.store, &iter)
            {
                // position is in iter
                self.selected_iter = Some(iter.clone());
                self.iter = Some(iter);
                return (true, prev_selected_iter);
            } else if position >= ChapterEntry::get_end(&self.store, &iter) && searching_forward {
                // position is after iter and we were already searching forward
                self.store.iter_next(&iter)
            } else if position < ChapterEntry::get_start(&self.store, &iter) {
                // position before iter
                searching_forward = false;
                if self.store.iter_previous(&iter) {
                    // iter is still valid
                    true
                } else {
                    // before first chapter
                    self.iter = self.store.get_iter_first();
                    return (has_changed, prev_selected_iter);
                }
            } else {
                // in a gap between two chapters
                self.iter = Some(iter);
                return (has_changed, prev_selected_iter);
            } {}

            // passed the end of the last chapter
            // prevent from searching in subsequent calls
            self.iter = None;
        }

        (has_changed, prev_selected_iter)
    }

    pub fn prepare_for_seek(&mut self) {
        if self.iter.is_none() {
            // either there is no chapter or previous position had already passed the end
            // => force scanning of the chapters list in order to set iter back on track
            self.rewind();
        }
    }
}
