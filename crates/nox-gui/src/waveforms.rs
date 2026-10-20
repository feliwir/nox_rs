//! Audio waveforms for the timeline's audio clips, computed in the
//! background (`nox_media::waveform`) and cached on disk under the user's
//! cache folder, so each file is analysed once.

use gtk::{gio, glib};
use nox_media::waveform::Waveform;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

enum Entry {
    Pending,
    Ready(Rc<Waveform>),
    /// No audio, or unreadable; not tried again.
    Failed,
}

pub struct Waveforms {
    /// By file and audio stream (`None`: the best one).
    entries: RefCell<HashMap<(PathBuf, Option<usize>), Entry>>,
    cache_dir: PathBuf,
    /// Stops the computations still running when this is dropped.
    cancel: Arc<AtomicBool>,
    /// Called when a waveform has arrived.
    on_ready: Box<dyn Fn()>,
}

impl Waveforms {
    pub fn new(on_ready: impl Fn() + 'static) -> Rc<Self> {
        Rc::new(Self {
            entries: RefCell::default(),
            cache_dir: glib::user_cache_dir().join("nox").join("waveforms"),
            cancel: Arc::default(),
            on_ready: Box::new(on_ready),
        })
    }

    /// The waveform of audio stream `stream` of `path` if it's ready;
    /// otherwise starts computing it (once) and calls `on_ready` when it
    /// is.
    pub fn get(self: &Rc<Self>, path: &Path, stream: Option<usize>) -> Option<Rc<Waveform>> {
        let key = (path.to_owned(), stream);
        match self.entries.borrow().get(&key) {
            Some(Entry::Ready(waveform)) => return Some(waveform.clone()),
            Some(_) => return None,
            None => {}
        }
        self.entries
            .borrow_mut()
            .insert(key.clone(), Entry::Pending);
        let (cache_dir, cancel) = (self.cache_dir.clone(), self.cancel.clone());
        let this = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let file = key.0.clone();
            let result = gio::spawn_blocking(move || {
                Waveform::load_or_compute(&file, stream, &cache_dir, &cancel)
            })
            .await;
            let Some(this) = this.upgrade() else {
                return;
            };
            let entry = match result {
                Ok(Ok(waveform)) => Entry::Ready(Rc::new(waveform)),
                Ok(Err(err)) => {
                    tracing::info!("no waveform for {}: {err}", key.0.display());
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

impl Drop for Waveforms {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
