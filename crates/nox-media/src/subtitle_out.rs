//! Writing subtitle streams: text events (ASS event lines, as FFmpeg's
//! decoders give them) encoded by FFmpeg's encoders into the format the
//! container takes, and bitmap events (PGS, VobSub, DVB) copied packet by
//! packet from their files, at new times.

use ffmpeg::{Dictionary, codec, ffi, format};
use nox_project::StreamTags;
use std::collections::VecDeque;
use std::path::PathBuf;

use crate::{BitmapReader, Container, Error, Result};

/// A text event to write: when it shows (seconds in the output) and its
/// ASS event line (`nox_ass::Event::from_ffmpeg`'s format).
#[derive(Debug, Clone, PartialEq)]
pub struct TextCue {
    pub start: f64,
    pub end: f64,
    pub ass: String,
}

/// A bitmap event to copy: when it shows in the output, and where it is:
/// the event starting at `source` (file time) of stream `stream` of
/// `path`.
#[derive(Debug, Clone, PartialEq)]
pub struct BitmapCue {
    pub start: f64,
    pub end: f64,
    pub path: PathBuf,
    pub stream: usize,
    pub source: f64,
}

/// What a subtitle stream holds.
#[derive(Debug, Clone, PartialEq)]
pub enum SubtitleContent {
    /// Text under an ASS script header (play size, styles). `codec` is the
    /// FFmpeg encoder wanted (`subrip`, `ass`, `webvtt`, `mov_text`);
    /// containers that can't take it get theirs.
    Text {
        codec: String,
        header: String,
        cues: Vec<TextCue>,
    },
    /// Pictures copied from their files (all of one codec).
    Bitmap { cues: Vec<BitmapCue> },
}

/// A subtitle stream to write.
#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleOutput {
    pub tags: StreamTags,
    pub content: SubtitleContent,
}

impl Container {
    /// The text subtitle format it holds for one that is `wanted` (an
    /// FFmpeg codec name), `None` if it holds none.
    pub fn text_subtitles(self, wanted: &str) -> Option<&'static str> {
        match self {
            Self::Mkv => Some(match wanted {
                "subrip" => "subrip",
                "webvtt" => "webvtt",
                // ASS keeps the styles of anything else.
                _ => "ass",
            }),
            Self::Mp4 | Self::Mov => Some("mov_text"),
            Self::Webm => Some("webvtt"),
            Self::Avi => None,
        }
    }

    /// Whether it holds bitmap subtitles (copied as they are).
    pub fn holds_bitmap_subtitles(self) -> bool {
        self == Self::Mkv
    }
}

/// A subtitle stream being written: its packets, in output time
/// (milliseconds), written as the video passes them.
pub(crate) struct SubtitleWriter {
    stream: usize,
    packets: VecDeque<(i64, i64, Vec<u8>)>,
}

/// Sets `stream`'s language and title tags and its dispositions.
pub(crate) fn tag_stream(stream: &mut format::stream::StreamMut, tags: &StreamTags) {
    let mut metadata = Dictionary::new();
    if let Some(language) = &tags.language {
        metadata.set("language", language);
    }
    if let Some(title) = &tags.title {
        metadata.set("title", title);
    }
    stream.set_metadata(metadata);
    let mut disposition = 0;
    for (on, flag) in [
        (tags.default, ffi::AV_DISPOSITION_DEFAULT),
        (tags.forced, ffi::AV_DISPOSITION_FORCED),
        (tags.hearing_impaired, ffi::AV_DISPOSITION_HEARING_IMPAIRED),
    ] {
        if on {
            disposition |= flag;
        }
    }
    // SAFETY: a field of the output's own stream, before the header.
    unsafe { (*stream.as_mut_ptr()).disposition = disposition };
}

/// Milliseconds.
fn ms(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

/// Adds a stream for `subtitles` to `output` (before its header) and
/// prepares its packets; `None` (and why) if `container` can't hold it.
pub(crate) fn open_subtitles(
    output: &mut format::context::Output,
    subtitles: &SubtitleOutput,
    container: Container,
) -> Result<std::result::Result<SubtitleWriter, String>> {
    match &subtitles.content {
        SubtitleContent::Text {
            codec,
            header,
            cues,
        } => {
            let Some(name) = container.text_subtitles(codec) else {
                return Ok(Err(format!(
                    "{} files can't hold subtitles",
                    container.name()
                )));
            };
            let found = ffmpeg::encoder::find_by_name(name)
                .ok_or_else(|| Error::Encode(format!("no {name} subtitle encoder")))?;
            let mut context = codec::Context::new_with_codec(found).encoder().subtitle()?;
            context.set_time_base(ffmpeg::Rational::new(1, 1000));
            // SAFETY: the unopened context takes an `av_malloc`ed copy of
            // the header, which it frees.
            unsafe {
                let ctx = context.as_mut_ptr();
                let bytes = header.as_bytes();
                let copy = ffi::av_mallocz(bytes.len() + 1) as *mut u8;
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), copy, bytes.len());
                (*ctx).subtitle_header = copy;
                (*ctx).subtitle_header_size = bytes.len() as i32;
                if output
                    .format()
                    .flags()
                    .contains(format::Flags::GLOBAL_HEADER)
                {
                    (*ctx).flags |= ffi::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
                }
            }
            let mut encoder = context.open_as(found)?;
            let mut stream = output.add_stream(found)?;
            stream.set_parameters(&*encoder);
            stream.set_time_base(ffmpeg::Rational::new(1, 1000));
            tag_stream(&mut stream, &subtitles.tags);
            let index = stream.index();
            let mut packets = VecDeque::new();
            let mut buffer = vec![0u8; 1 << 16];
            for (order, cue) in cues.iter().enumerate() {
                let (start, end) = (ms(cue.start), ms(cue.end));
                if end <= start {
                    continue;
                }
                // ReadOrder first, as FFmpeg's decoders write it.
                let line = match cue.ass.split_once(',') {
                    Some((_, rest)) => format!("{order},{rest}"),
                    None => continue,
                };
                let mut subtitle = ffmpeg::Subtitle::new();
                subtitle.add_rect(codec::subtitle::Type::Ass);
                if let Some(codec::subtitle::RectMut::Ass(mut rect)) = subtitle.rects_mut().next() {
                    rect.set(&line.replace('\0', ""));
                }
                subtitle.set_pts(Some(start * 1000));
                subtitle.set_start(0);
                subtitle.set_end((end - start) as u32);
                // SAFETY: an open encoder, a filled subtitle (freed after),
                // and a buffer of the size given.
                let size = unsafe {
                    let size = ffi::avcodec_encode_subtitle(
                        encoder.as_mut_ptr(),
                        buffer.as_mut_ptr(),
                        buffer.len() as i32,
                        subtitle.as_ptr(),
                    );
                    ffi::avsubtitle_free(subtitle.as_mut_ptr());
                    size
                };
                if size < 0 {
                    tracing::debug!("subtitle at {start} ms not encoded: {size}");
                    continue;
                }
                packets.push_back((start, end - start, buffer[..size as usize].to_vec()));
            }
            Ok(Ok(SubtitleWriter {
                stream: index,
                packets,
            }))
        }
        SubtitleContent::Bitmap { cues } => {
            if !container.holds_bitmap_subtitles() {
                return Ok(Err(format!(
                    "{} files can't hold bitmap subtitles",
                    container.name()
                )));
            }
            let Some(first) = cues.first() else {
                return Ok(Err("no subtitles".into()));
            };
            let mut readers: Vec<BitmapReader> = Vec::new();
            // The reader of a stream, opened once.
            fn reader_for(
                readers: &mut Vec<BitmapReader>,
                path: &PathBuf,
                stream: usize,
            ) -> Result<usize> {
                if let Some(i) = readers
                    .iter()
                    .position(|r| r.path() == path && r.stream_index() == stream)
                {
                    return Ok(i);
                }
                readers.push(BitmapReader::open(path, stream)?);
                Ok(readers.len() - 1)
            }
            let first_reader = reader_for(&mut readers, &first.path, first.stream)?;
            let parameters = readers[first_reader].parameters();
            let codec_id = parameters.id();
            let mut stream = output.add_stream(ffmpeg::encoder::find(codec::Id::None))?;
            stream.set_parameters(parameters);
            // SAFETY: the new stream's own parameters: no tag of the
            // source container's.
            unsafe { (*(*stream.as_mut_ptr()).codecpar).codec_tag = 0 };
            stream.set_time_base(ffmpeg::Rational::new(1, 1000));
            tag_stream(&mut stream, &subtitles.tags);
            let index = stream.index();
            let mut packets = VecDeque::new();
            for cue in cues {
                let i = reader_for(&mut readers, &cue.path, cue.stream)?;
                if readers[i].parameters().id() != codec_id {
                    tracing::warn!(
                        "{}: other subtitle format than the track's first, left out",
                        cue.path.display()
                    );
                    continue;
                }
                let Some(data) = readers[i].packet(cue.source)? else {
                    tracing::debug!(
                        "no subtitle at {:.3} s of {}",
                        cue.source,
                        cue.path.display()
                    );
                    continue;
                };
                let (start, end) = (ms(cue.start), ms(cue.end));
                if end <= start {
                    continue;
                }
                // PGS ends a subtitle with the next display set, not its
                // duration: one clearing the screen.
                let clear = (codec_id == codec::Id::HDMV_PGS_SUBTITLE)
                    .then(|| pgs_clear(&data))
                    .flatten();
                packets.push_back((start, end - start, data));
                if let Some(clear) = clear {
                    packets.push_back((end, 0, clear));
                }
            }
            // In time order (a clear may come after the next start).
            let mut sorted: Vec<_> = packets.into_iter().collect();
            sorted.sort_by_key(|p| p.0);
            Ok(Ok(SubtitleWriter {
                stream: index,
                packets: sorted.into(),
            }))
        }
    }
}

/// A PGS display set clearing what `shown` (a display set as Matroska
/// stores it: segments of type, size and payload) showed: its
/// presentation without objects, and an end.
fn pgs_clear(shown: &[u8]) -> Option<Vec<u8>> {
    let (kind, rest) = shown.split_first()?;
    let size = u16::from_be_bytes([*rest.first()?, *rest.get(1)?]) as usize;
    let payload = rest.get(2..2 + size)?;
    // The presentation composition comes first: size, frame rate,
    // number, state, palette, objects.
    if *kind != 0x16 || payload.len() < 11 {
        return None;
    }
    let number = u16::from_be_bytes([payload[5], payload[6]]).wrapping_add(1);
    let mut composition = payload[..5].to_vec();
    composition.extend_from_slice(&number.to_be_bytes());
    // Normal state, no palette update, its palette, no objects.
    composition.extend_from_slice(&[0x00, 0x00, payload[9], 0x00]);
    let mut out = vec![0x16];
    out.extend_from_slice(&(composition.len() as u16).to_be_bytes());
    out.extend_from_slice(&composition);
    out.extend_from_slice(&[0x80, 0x00, 0x00]);
    Some(out)
}

impl SubtitleWriter {
    /// Writes the packets that start before `seconds` (all with `None`).
    pub(crate) fn write_until(
        &mut self,
        output: &mut format::context::Output,
        seconds: Option<f64>,
    ) -> Result<()> {
        let limit = seconds.map_or(i64::MAX, ms);
        let time_base = output.stream(self.stream).expect("stream").time_base();
        while self.packets.front().is_some_and(|p| p.0 < limit) {
            let (start, duration, data) = self.packets.pop_front().expect("packet");
            let mut packet = ffmpeg::Packet::copy(&data);
            packet.set_stream(self.stream);
            packet.set_pts(Some(start));
            packet.set_dts(Some(start));
            packet.set_duration(duration);
            packet.set_flags(codec::packet::Flags::KEY);
            packet.rescale_ts(ffmpeg::Rational::new(1, 1000), time_base);
            packet.write_interleaved(output)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn containers_take_formats() {
        assert_eq!(Container::Mkv.text_subtitles("subrip"), Some("subrip"));
        assert_eq!(Container::Mkv.text_subtitles("mov_text"), Some("ass"));
        assert_eq!(Container::Mkv.text_subtitles("ass"), Some("ass"));
        assert_eq!(Container::Mp4.text_subtitles("ass"), Some("mov_text"));
        assert_eq!(Container::Webm.text_subtitles("subrip"), Some("webvtt"));
        assert_eq!(Container::Avi.text_subtitles("subrip"), None);
        assert!(Container::Mkv.holds_bitmap_subtitles());
        assert!(!Container::Mp4.holds_bitmap_subtitles());
    }

    #[test]
    fn clears_pgs() {
        // Presentation: 1920×1080, rate 0x10, number 7, epoch start,
        // palette 0 (id 2), one object; then an end segment.
        let mut shown = vec![0x16, 0x00, 19];
        shown.extend_from_slice(&[7, 0x80, 4, 0x38, 0x10, 0, 7, 0x80, 0, 2, 1]);
        shown.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
        shown.extend_from_slice(&[0x80, 0, 0]);
        let clear = pgs_clear(&shown).unwrap();
        assert_eq!(
            clear,
            [
                0x16, 0, 11, 7, 0x80, 4, 0x38, 0x10, 0, 8, 0, 0, 2, 0, 0x80, 0, 0
            ]
        );
        assert_eq!(pgs_clear(&[0x80, 0, 0]), None);
    }
}
