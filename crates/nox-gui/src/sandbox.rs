//! Paths inside the Flatpak sandbox. Files from outside the folders Nox
//! may read (file chooser, drag and drop, "Open With") come through the
//! document portal as `/run/user/<uid>/doc/<id>/<name>`, which means
//! nothing outside the sandbox. Projects store the files' host paths
//! instead, asked from the portal when saving; a map of the ones seen, in
//! the app's data folder, turns them back when opening. Outside Flatpak
//! paths stay as they are.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use gtk::prelude::*;
use gtk::{gio, glib};
use nox_project::timeline::file_url;
use nox_project::{MediaReference, ReferenceKind, Timeline};

static FLATPAK: LazyLock<bool> = LazyLock::new(|| Path::new("/.flatpak-info").exists());

/// Whether Nox runs in Flatpak.
pub fn is_flatpak() -> bool {
    *FLATPAK
}

/// Where the document portal's files are mounted.
fn documents() -> PathBuf {
    glib::user_runtime_dir().join("doc")
}

/// Host path to document path, for the files seen.
#[derive(Default)]
struct Known {
    paths: HashMap<PathBuf, PathBuf>,
    loaded: bool,
}

thread_local! {
    static KNOWN: RefCell<Known> = RefCell::default();
}

fn known_file() -> PathBuf {
    glib::user_data_dir().join("nox").join("documents")
}

/// The map's file: a host path and a document path per line, tab
/// separated (paths with tabs or newlines aren't kept).
fn with_known<R>(f: impl FnOnce(&mut HashMap<PathBuf, PathBuf>) -> R) -> R {
    KNOWN.with_borrow_mut(|known| {
        if !known.loaded {
            known.loaded = true;
            if let Ok(text) = std::fs::read_to_string(known_file()) {
                known.paths = text
                    .lines()
                    .filter_map(|line| line.split_once('\t'))
                    .map(|(host, doc)| (host.into(), doc.into()))
                    .collect();
            }
        }
        f(&mut known.paths)
    })
}

fn remember(pairs: &[(PathBuf, PathBuf)]) {
    let changed = with_known(|known| {
        let mut changed = false;
        for (doc, host) in pairs {
            if known.get(host) != Some(doc) {
                known.insert(host.clone(), doc.clone());
                changed = true;
            }
        }
        changed.then(|| {
            known
                .iter()
                .filter_map(|(host, doc)| Some((host.to_str()?, doc.to_str()?)))
                .filter(|(host, doc)| !host.contains(['\t', '\n']) && !doc.contains(['\t', '\n']))
                .map(|(host, doc)| format!("{host}\t{doc}\n"))
                .collect::<String>()
        })
    });
    if let Some(text) = changed {
        let file = known_file();
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&file, text));
        if let Err(err) = written {
            tracing::warn!("cannot write {}: {err}", file.display());
        }
    }
}

/// The host paths of those of `paths` that are document portal files,
/// keyed by their document path. Others aren't in the map.
pub fn host_paths<'a>(paths: impl IntoIterator<Item = &'a Path>) -> HashMap<PathBuf, PathBuf> {
    if !*FLATPAK {
        return HashMap::new();
    }
    let documents = documents();
    // Document id, then the path below the document's own file or folder.
    let in_portal: Vec<(&Path, String, PathBuf)> = paths
        .into_iter()
        .filter_map(|path| {
            let mut parts = path.strip_prefix(&documents).ok()?.components();
            let id = parts.next()?.as_os_str().to_str()?.to_owned();
            parts.next()?;
            Some((path, id, parts.as_path().to_owned()))
        })
        .collect();
    if in_portal.is_empty() {
        return HashMap::new();
    }
    let mut ids: Vec<&str> = in_portal.iter().map(|(_, id, _)| id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    let hosts = match get_host_paths(&ids) {
        Ok(hosts) => hosts,
        Err(err) => {
            // Before xdg-desktop-portal 1.18: the projects keep the
            // document paths, valid as long as Nox may open the files.
            tracing::warn!("cannot ask the document portal for host paths: {err}");
            return HashMap::new();
        }
    };
    let pairs: Vec<(PathBuf, PathBuf)> = in_portal
        .into_iter()
        .filter_map(|(path, id, rest)| {
            let host = hosts.get(&id)?;
            Some((
                path.to_owned(),
                if rest.as_os_str().is_empty() {
                    host.clone()
                } else {
                    host.join(rest)
                },
            ))
        })
        .collect();
    remember(&pairs);
    pairs.into_iter().collect()
}

/// `path` as it is outside the sandbox.
pub fn host_path(path: &Path) -> PathBuf {
    host_paths([path])
        .remove(path)
        .unwrap_or_else(|| path.to_owned())
}

/// Where Nox can open the file at `host`, if not there: the document path
/// it had when last seen.
pub fn sandbox_path(host: &Path) -> Option<PathBuf> {
    if !*FLATPAK || host.exists() {
        return None;
    }
    with_known(|known| known.get(host).filter(|doc| doc.exists()).cloned())
}

/// `org.freedesktop.portal.Documents.GetHostPaths`: document id to path.
#[cfg(unix)]
fn get_host_paths(ids: &[&str]) -> Result<HashMap<String, PathBuf>, glib::Error> {
    use std::os::unix::ffi::OsStrExt;
    let bus = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
    let reply = bus.call_sync(
        Some("org.freedesktop.portal.Documents"),
        "/org/freedesktop/portal/documents",
        "org.freedesktop.portal.Documents",
        "GetHostPaths",
        Some(&(ids.to_vec(),).to_variant()),
        Some(glib::VariantTy::new("(a{say})").expect("reply type")),
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
    )?;
    let paths = reply.child_value(0);
    Ok(paths
        .iter()
        .filter_map(|entry| {
            let id = entry.child_value(0).str()?.to_owned();
            let bytes = entry.child_value(1);
            let bytes = bytes.fixed_array::<u8>().ok()?;
            // A bytestring: with its trailing NUL.
            let bytes = bytes.strip_suffix(&[0]).unwrap_or(bytes);
            Some((id, Path::new(std::ffi::OsStr::from_bytes(bytes)).to_owned()))
        })
        .collect())
}

#[cfg(not(unix))]
fn get_host_paths(_: &[&str]) -> Result<HashMap<String, PathBuf>, glib::Error> {
    Ok(HashMap::new())
}

/// Points every file reference whose (resolved) path `map` gives a new one
/// for at that one.
pub fn remap(timeline: &mut Timeline, map: impl Fn(&Path) -> Option<PathBuf>) {
    let dir = timeline.media_dir.clone();
    timeline.for_each_reference_mut(|reference: &mut MediaReference| {
        let Some(path) = resolved(reference, dir.as_deref()) else {
            return;
        };
        if let Some(new) = map(&path)
            && let ReferenceKind::External { target_url } = &mut reference.kind
        {
            *target_url = file_url(&new);
        }
    });
}

/// Points `timeline`'s references to document portal files at the files'
/// host paths, for saving.
pub fn use_host_paths(timeline: &mut Timeline) {
    let dir = timeline.media_dir.clone();
    let mut paths = Vec::new();
    timeline.for_each_reference_mut(|reference| paths.extend(resolved(reference, dir.as_deref())));
    let hosts = host_paths(paths.iter().map(PathBuf::as_path));
    if !hosts.is_empty() {
        remap(timeline, |path| hosts.get(path).cloned());
    }
}

/// The local file of `reference`, a relative path resolved against `dir`
/// (as `Timeline::media_path`).
fn resolved(reference: &MediaReference, dir: Option<&Path>) -> Option<PathBuf> {
    let path = reference.path()?;
    Some(match dir {
        Some(dir) if path.is_relative() => dir.join(path),
        _ => path,
    })
}
