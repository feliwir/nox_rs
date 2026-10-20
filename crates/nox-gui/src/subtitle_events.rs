//! The events of subtitle streams for the timeline's subtitle clips,
//! read in the background (`nox_media::subtitles`, which the engine then
//! finds already read) with each event's text, as the clips show it.

use gtk::{gio, glib};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// A stream's events: when each shows (file seconds) and its text (empty
/// for pictures).
pub struct Events {
    pub events: Vec<(f64, f64, String)>,
}

enum Entry {
    Pending,
    Ready(Rc<Events>),
    Failed,
}

pub struct SubtitleEvents {
    entries: RefCell<HashMap<(PathBuf, usize), Entry>>,
    /// Called when a stream's events have arrived.
    on_ready: Box<dyn Fn()>,
}

/// Reads the events of stream `stream` of `path`, with their text.
fn read(path: &Path, stream: usize) -> nox_media::Result<Events> {
    let events = match &*nox_media::subtitles(path, stream)? {
        nox_media::Subtitles::Text { header, events } => {
            let script = nox_ass::Script::parse(header);
            events
                .iter()
                .map(|e| {
                    let text = nox_ass::Event::from_ffmpeg(&e.ass, e.start, e.end)
                        .and_then(|event| script.caption(&event))
                        .map(|c| {
                            c.document
                                .text
                                .split_whitespace()
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default();
                    (e.start, e.end, text)
                })
                .collect()
        }
        nox_media::Subtitles::Bitmap { events } => {
            events.iter().map(|&(s, e)| (s, e, String::new())).collect()
        }
    };
    Ok(Events { events })
}

impl SubtitleEvents {
    pub fn new(on_ready: impl Fn() + 'static) -> Rc<Self> {
        Rc::new(Self {
            entries: RefCell::default(),
            on_ready: Box::new(on_ready),
        })
    }

    /// The events of stream `stream` of `path` if they've been read;
    /// otherwise starts reading them (once) and calls `on_ready` when
    /// they're there.
    pub fn get(self: &Rc<Self>, path: &Path, stream: usize) -> Option<Rc<Events>> {
        let key = (path.to_owned(), stream);
        match self.entries.borrow().get(&key) {
            Some(Entry::Ready(events)) => return Some(events.clone()),
            Some(_) => return None,
            None => {}
        }
        self.entries
            .borrow_mut()
            .insert(key.clone(), Entry::Pending);
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let file = key.0.clone();
            let result = gio::spawn_blocking(move || read(&file, stream)).await;
            let Some(this) = this.upgrade() else {
                return;
            };
            let entry = match result {
                Ok(Ok(events)) => Entry::Ready(Rc::new(events)),
                Ok(Err(err)) => {
                    tracing::info!("no subtitles in {} #{stream}: {err}", key.0.display());
                    Entry::Failed
                }
                Err(_) => Entry::Failed,
            };
            this.entries.borrow_mut().insert(key, entry);
            (this.on_ready)();
        });
        None
    }
}
