//! Subtitle streams: listing them, reading a text stream's events (as
//! FFmpeg decodes every text format, SRT, WebVTT, mov_text and ASS
//! alike: ASS event lines under an ASS header, which nox-ass reads), and
//! bitmap subtitles (PGS, VobSub, DVB) as pictures.
//!
//! Times are in the file's time, as `VideoDecoder`'s: from the start of
//! its video stream (of the file's start without one).

use ffmpeg::{codec, ffi, format, media};
use nox_project::StreamTags;
use std::collections::HashMap;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use crate::{Error, Result};

/// A subtitle stream of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleInfo {
    pub stream_index: usize,
    /// FFmpeg's codec name: `subrip`, `ass`, `hdmv_pgs_subtitle`, ….
    pub codec: String,
    /// Pictures (PGS, VobSub, DVB), not text.
    pub bitmap: bool,
    /// Its language (ISO 639-2, e.g. `eng`), title and flags.
    pub tags: StreamTags,
}

impl SubtitleInfo {
    /// A name to show for it: title, language and format.
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if let Some(title) = &self.tags.title {
            parts.push(title);
        }
        if let Some(language) = &self.tags.language {
            parts.push(language);
        }
        format!("{} ({})", parts.join(", "), format_name(&self.codec))
            .trim_start()
            .to_owned()
    }
}

/// A subtitle format's usual name.
pub fn format_name(codec: &str) -> &str {
    match codec {
        "subrip" => "SRT",
        "ass" | "ssa" => "ASS",
        "webvtt" => "WebVTT",
        "mov_text" => "MP4 text",
        "hdmv_pgs_subtitle" => "PGS",
        "dvd_subtitle" => "VobSub",
        "dvb_subtitle" => "DVB",
        other => other,
    }
}

/// A stream's language, title and flags, as its container tags it.
pub(crate) fn stream_tags(stream: &format::stream::Stream) -> StreamTags {
    let metadata = stream.metadata();
    let tag = |key: &str| {
        metadata
            .get(key)
            .map(str::trim)
            .filter(|v| !v.is_empty() && *v != "und")
            .map(str::to_owned)
    };
    use format::stream::Disposition;
    let disposition = stream.disposition();
    StreamTags {
        language: tag("language"),
        title: tag("title"),
        default: disposition.contains(Disposition::DEFAULT),
        forced: disposition.contains(Disposition::FORCED),
        hearing_impaired: disposition.contains(Disposition::HEARING_IMPAIRED),
    }
}

/// A text event: when it shows (seconds, file time), and FFmpeg's ASS
/// event line for it (`nox_ass::Event::from_ffmpeg`).
#[derive(Debug, Clone, PartialEq)]
pub struct TextEvent {
    pub start: f64,
    pub end: f64,
    pub ass: String,
}

/// A stream's events.
#[derive(Debug, Clone, PartialEq)]
pub enum Subtitles {
    /// Text, under an ASS script header (its play size and styles).
    Text {
        header: String,
        events: Vec<TextEvent>,
    },
    /// Pictures: when each shows (seconds, file time); `BitmapReader`
    /// draws them.
    Bitmap { events: Vec<(f64, f64)> },
}

/// A bitmap subtitle drawn: straight-alpha sRGB RGBA8, `width` ×
/// `height`, at (`x`, `y`) on a canvas of `canvas` size, which is laid
/// over the video as a picture of that size would be.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitlePicture {
    pub canvas: (u32, u32),
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// How long an event without an end shows at most (the next one ends it
/// sooner).
const MAX_EVENT: f64 = 10.0;

/// The subtitle streams of `input` that FFmpeg can decode.
pub(crate) fn subtitle_streams(input: &format::context::Input) -> Vec<SubtitleInfo> {
    input
        .streams()
        .filter(|s| s.parameters().medium() == media::Type::Subtitle)
        .filter_map(|stream| {
            let id = stream.parameters().id();
            if ffmpeg::decoder::find(id).is_none() {
                tracing::debug!("no decoder for subtitle stream {}", stream.index());
                return None;
            }
            Some(SubtitleInfo {
                stream_index: stream.index(),
                codec: id.name().to_owned(),
                bitmap: is_bitmap(id),
                tags: stream_tags(&stream),
            })
        })
        .collect()
}

/// Whether codec `id` decodes to pictures.
fn is_bitmap(id: codec::Id) -> bool {
    // SAFETY: descriptors are static.
    let props = unsafe {
        let descriptor = ffi::avcodec_descriptor_get(id.into());
        if descriptor.is_null() {
            return false;
        }
        (*descriptor).props
    };
    props & ffi::AV_CODEC_PROP_BITMAP_SUB != 0 && props & ffi::AV_CODEC_PROP_TEXT_SUB == 0
}

/// Where file time 0 is in the container's time (seconds): the video
/// stream's start, as `VideoDecoder` counts.
fn origin(input: &format::context::Input) -> f64 {
    input
        .streams()
        .best(media::Type::Video)
        .and_then(|s| {
            let start = crate::stream::start_time(&s);
            crate::rational(s.time_base()).map(|tb| start as f64 * tb.as_f64())
        })
        .unwrap_or(0.0)
}

/// An open subtitle stream.
struct Stream {
    input: format::context::Input,
    decoder: ffmpeg::decoder::Subtitle,
    index: usize,
    /// Of the stream, seconds per tick.
    time_base: f64,
    origin: f64,
    /// Of the file's video stream, if any.
    video_size: Option<(u32, u32)>,
    /// The packets read since the last subtitle came out, which make the
    /// next one (a PGS display set in a `.sup` is a packet per segment).
    pending: Vec<u8>,
}

/// A subtitle read: its start, its end (if known), the subtitle and the
/// packet data it came from (`Stream::next`).
type Next = (f64, Option<f64>, Decoded, Vec<u8>);

/// A decoded subtitle, freed when dropped.
struct Decoded(ffmpeg::Subtitle);

impl Drop for Decoded {
    fn drop(&mut self) {
        // SAFETY: filled by the decoder (or zeroed); freeing twice is safe
        // as `avsubtitle_free` clears it.
        unsafe { ffi::avsubtitle_free(self.0.as_mut_ptr()) };
    }
}

/// Unknown display end (as PGS gives): ends with the next event.
fn known_end(ms: u32) -> Option<f64> {
    (ms != 0 && ms != u32::MAX).then(|| f64::from(ms) / 1000.0)
}

impl Stream {
    fn open(path: &Path, index: usize) -> Result<Self> {
        crate::init();
        let input = format::input(path)?;
        let stream = input
            .stream(index)
            .filter(|s| s.parameters().medium() == media::Type::Subtitle)
            .ok_or(Error::NoSubtitleStream)?;
        let time_base = crate::rational(stream.time_base())
            .map(|r| r.as_f64())
            .unwrap_or(1.0 / 1000.0);
        let mut decoder = codec::Context::from_parameters(stream.parameters())?.decoder();
        // Text decoders time their events by it.
        decoder.set_packet_time_base(stream.time_base());
        let decoder = decoder.subtitle()?;
        let video_size = input.streams().best(media::Type::Video).and_then(|s| {
            let parameters = s.parameters();
            // SAFETY: the stream's live parameters.
            let (w, h) = unsafe { ((*parameters.as_ptr()).width, (*parameters.as_ptr()).height) };
            (w > 0 && h > 0).then_some((w as u32, h as u32))
        });
        Ok(Self {
            origin: origin(&input),
            input,
            decoder,
            index,
            time_base,
            video_size,
            pending: Vec::new(),
        })
    }

    /// The next subtitle of the stream decoded: its start and end
    /// (seconds, file time; the end `None` if unknown), the subtitle and
    /// the packet data it was decoded from. `None` at the end.
    fn next(&mut self) -> Result<Option<Next>> {
        let mut packet = ffmpeg::Packet::empty();
        loop {
            match packet.read(&mut self.input) {
                Ok(()) if packet.stream() == self.index => {}
                Ok(()) => continue,
                Err(ffmpeg::Error::Eof) => return Ok(None),
                Err(err) => return Err(err.into()),
            }
            let Some(pts) = packet.pts().or(packet.dts()) else {
                continue;
            };
            let at = pts as f64 * self.time_base - self.origin;
            self.pending
                .extend_from_slice(packet.data().unwrap_or_default());
            let mut decoded = Decoded(ffmpeg::Subtitle::new());
            match self.decoder.decode(&packet, &mut decoded.0) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(err) => {
                    tracing::debug!("subtitle packet at {at:.3} s: {err}");
                    self.pending.clear();
                    continue;
                }
            }
            let data = std::mem::take(&mut self.pending);
            // The subtitle's own time where the decoder gives one (PGS:
            // its presentation's, not that of the packet completing it).
            let shown = decoded.0.pts().map_or(at, |pts| {
                pts as f64 / f64::from(ffi::AV_TIME_BASE) - self.origin
            });
            let start = shown + f64::from(decoded.0.start()) / 1000.0;
            let end = known_end(decoded.0.end())
                .map(|end| shown + end)
                .or_else(|| {
                    (packet.duration() > 0).then(|| at + packet.duration() as f64 * self.time_base)
                })
                .filter(|&end| end > start);
            return Ok(Some((start, end, decoded, data)));
        }
    }

    /// The ASS header the decoder writes for text streams.
    fn header(&self) -> String {
        // SAFETY: an open decoder; the header, if any, is
        // `subtitle_header_size` bytes.
        unsafe {
            let ctx = self.decoder.as_ptr();
            let (data, size) = ((*ctx).subtitle_header, (*ctx).subtitle_header_size);
            if data.is_null() || size <= 0 {
                return String::new();
            }
            String::from_utf8_lossy(std::slice::from_raw_parts(data, size as usize)).into_owned()
        }
    }

    /// The size bitmaps are placed on: the decoder's (PGS and VobSub say),
    /// else the video's, else DVB's default.
    fn canvas(&self) -> (u32, u32) {
        // SAFETY: an open decoder.
        let (w, h) = unsafe {
            let ctx = self.decoder.as_ptr();
            ((*ctx).width, (*ctx).height)
        };
        if w > 0 && h > 0 {
            (w as u32, h as u32)
        } else {
            self.video_size.unwrap_or((720, 576))
        }
    }
}

/// `rect`'s plain text as an ASS event line (FFmpeg's event format).
fn text_line(text: &str) -> String {
    let escaped = text.trim_end_matches('\n').replace('\n', "\\N");
    format!("0,0,Default,,0,0,0,,{escaped}")
}

/// Reads all events of subtitle stream `index` of `path`. Events of
/// unknown length end at the next one (at most `MAX_EVENT` later).
pub fn read_subtitles(path: &Path, index: usize) -> Result<Subtitles> {
    let mut stream = Stream::open(path, index)?;
    let bitmap = is_bitmap(stream.decoder.id());
    let mut text: Vec<TextEvent> = Vec::new();
    let mut shown: Vec<(f64, Option<f64>)> = Vec::new();
    // For bitmaps: whether the last event still shows (a subtitle
    // without pictures clears it).
    while let Some((start, end, decoded, _)) = stream.next()? {
        // SAFETY: the rects of a decoded subtitle.
        let rects: Vec<codec::subtitle::Rect> = decoded.0.rects().collect();
        if bitmap {
            if let Some(last) = shown.last_mut().filter(|l| l.1.is_none()) {
                last.1 = Some(start);
            }
            if rects
                .iter()
                .any(|r| matches!(r, codec::subtitle::Rect::Bitmap(_)))
            {
                shown.push((start, end));
            }
            continue;
        }
        for rect in rects {
            let ass = match rect {
                // SAFETY: the rect's own NUL-terminated strings.
                codec::subtitle::Rect::Ass(ass) => unsafe {
                    CStr::from_ptr((*ass.as_ptr()).ass)
                        .to_string_lossy()
                        .into_owned()
                },
                codec::subtitle::Rect::Text(t) => {
                    text_line(&unsafe { CStr::from_ptr((*t.as_ptr()).text).to_string_lossy() })
                }
                _ => continue,
            };
            text.push(TextEvent {
                start,
                end: end.unwrap_or(f64::NAN),
                ass,
            });
        }
    }
    let ends = |starts: Vec<f64>, ends: Vec<Option<f64>>| -> Vec<f64> {
        (0..starts.len())
            .map(|i| {
                let next = starts.get(i + 1).copied().filter(|&n| n > starts[i]);
                ends[i]
                    .or(next.map(|n| n.min(starts[i] + MAX_EVENT)))
                    .unwrap_or(starts[i] + MAX_EVENT.min(5.0))
            })
            .collect()
    };
    Ok(if bitmap {
        let starts: Vec<f64> = shown.iter().map(|e| e.0).collect();
        let fixed = ends(starts.clone(), shown.iter().map(|e| e.1).collect());
        Subtitles::Bitmap {
            events: starts.into_iter().zip(fixed).collect(),
        }
    } else {
        let starts: Vec<f64> = text.iter().map(|e| e.start).collect();
        let fixed = ends(
            starts,
            text.iter()
                .map(|e| Some(e.end).filter(|e| !e.is_nan()))
                .collect(),
        );
        for (event, end) in text.iter_mut().zip(fixed) {
            event.end = end;
        }
        Subtitles::Text {
            header: stream.header(),
            events: text,
        }
    })
}

/// Streams read so far, by file and stream, with the file's size and
/// modification time when read.
type Cache = HashMap<(PathBuf, usize), (Option<(u64, SystemTime)>, Arc<Subtitles>)>;

/// The events of subtitle stream `index` of `path` (`read_subtitles`),
/// read once per process and file version: the GUI and the engine share
/// them. Reading the first time takes a pass over the file.
pub fn subtitles(path: &Path, index: usize) -> Result<Arc<Subtitles>> {
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(Mutex::default);
    let version = std::fs::metadata(path)
        .ok()
        .and_then(|m| Some((m.len(), m.modified().ok()?)));
    let key = (path.to_owned(), index);
    if let Some((seen, subtitles)) = cache.lock().expect("cache").get(&key)
        && *seen == version
    {
        return Ok(subtitles.clone());
    }
    // Read without the lock: other streams stay available meanwhile.
    let subtitles = Arc::new(read_subtitles(path, index)?);
    cache
        .lock()
        .expect("cache")
        .insert(key, (version, subtitles.clone()));
    Ok(subtitles)
}

/// Draws a bitmap stream's subtitles, one event at a time (for the
/// engine, which shows them as layers).
pub struct BitmapReader {
    path: PathBuf,
    stream: Stream,
}

impl BitmapReader {
    pub fn open(path: &Path, index: usize) -> Result<Self> {
        Ok(Self {
            path: path.to_owned(),
            stream: Stream::open(path, index)?,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn stream_index(&self) -> usize {
        self.stream.index
    }

    /// The subtitle that starts at `start` (seconds, file time, as
    /// `read_subtitles` gave it); `None` if none does.
    pub fn picture(&mut self, start: f64) -> Result<Option<SubtitlePicture>> {
        Ok(self
            .find(start)?
            .and_then(|(decoded, _)| self.draw(&decoded)))
    }

    /// The packet data of the subtitle that starts at `start`, as stored
    /// (one packet, or a PGS display set's segments together), for copying
    /// it into another file.
    pub fn packet(&mut self, start: f64) -> Result<Option<Vec<u8>>> {
        Ok(self.find(start)?.map(|(_, data)| data))
    }

    /// The subtitle with pictures that starts at `start`, and its data.
    fn find(&mut self, start: f64) -> Result<Option<(Decoded, Vec<u8>)>> {
        // From a little before: the decoder may need what comes first
        // (PGS palettes and objects).
        let target = start + self.stream.origin - 1.0;
        let ts = (target.max(0.0) * f64::from(ffi::AV_TIME_BASE)) as i64;
        if self.stream.input.seek(ts, ..ts + 1).is_ok() {
            // SAFETY: an open decoder.
            unsafe { ffi::avcodec_flush_buffers(self.stream.decoder.as_mut_ptr()) };
            self.stream.pending.clear();
        } else {
            // Files that can't seek: from the start.
            self.stream = Stream::open(&self.path, self.stream.index)?;
        }
        while let Some((at, _, decoded, data)) = self.stream.next()? {
            if at < start - 0.002 {
                continue;
            }
            if at > start + 0.002 {
                return Ok(None);
            }
            let pictures = decoded
                .0
                .rects()
                .any(|r| matches!(r, codec::subtitle::Rect::Bitmap(_)));
            if pictures {
                return Ok(Some((decoded, data)));
            }
        }
        Ok(None)
    }

    /// The codec parameters of the stream, for a stream copying it.
    pub fn parameters(&self) -> codec::Parameters {
        self.stream
            .input
            .stream(self.stream.index)
            .expect("the stream")
            .parameters()
    }

    /// The subtitle's pictures, palette colors looked up, in one picture
    /// spanning them.
    fn draw(&self, decoded: &Decoded) -> Option<SubtitlePicture> {
        let mut rects = Vec::new();
        for rect in decoded.0.rects() {
            if let codec::subtitle::Rect::Bitmap(bitmap) = rect {
                // SAFETY: the decoder's rect.
                rects.push(unsafe { &*bitmap.as_ptr() });
            }
        }
        let rects: Vec<&ffi::AVSubtitleRect> = rects
            .into_iter()
            .filter(|r| r.w > 0 && r.h > 0 && !r.data[0].is_null() && !r.data[1].is_null())
            .collect();
        let x0 = rects.iter().map(|r| r.x).min()?;
        let y0 = rects.iter().map(|r| r.y).min()?;
        let x1 = rects.iter().map(|r| r.x + r.w).max()?;
        let y1 = rects.iter().map(|r| r.y + r.h).max()?;
        let (width, height) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut rgba = vec![0u8; width * height * 4];
        for r in rects {
            let colors = r.nb_colors.clamp(0, 256) as usize;
            // SAFETY: `data[1]` holds the palette (`nb_colors` native
            // 0xAARRGGBB words), `data[0]` `h` rows of `linesize[0]`
            // indices.
            let palette = unsafe { std::slice::from_raw_parts(r.data[1] as *const u32, colors) };
            for row in 0..r.h as usize {
                let indices = unsafe {
                    std::slice::from_raw_parts(
                        r.data[0].add(row * r.linesize[0] as usize),
                        r.w as usize,
                    )
                };
                let y = (r.y - y0) as usize + row;
                for (column, &index) in indices.iter().enumerate() {
                    let Some(&argb) = palette.get(usize::from(index)) else {
                        continue;
                    };
                    let x = (r.x - x0) as usize + column;
                    let at = (y * width + x) * 4;
                    rgba[at..at + 4].copy_from_slice(&[
                        (argb >> 16) as u8,
                        (argb >> 8) as u8,
                        argb as u8,
                        (argb >> 24) as u8,
                    ]);
                }
            }
        }
        Some(SubtitlePicture {
            canvas: self.stream.canvas(),
            x: x0,
            y: y0,
            width: width as u32,
            height: height as u32,
            rgba,
        })
    }
}
