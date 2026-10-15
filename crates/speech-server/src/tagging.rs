//! Tags on the files the studio writes (YuE2 Studio `crates/music-server/src/tagging.rs`, ce206f1).
//!
//! Tags go in each format's own place, written by lofty: ID3v2.4 on an MP3,
//! Vorbis comments with a picture block on a FLAC, RIFF INFO and an id3 chunk
//! on a WAV.

use std::path::Path;

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{FileType, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::prelude::{Accessor, ItemKey, TagExt};
use lofty::probe::Probe;
use lofty::tag::Tag;

/// What a player should show for one track.
#[derive(Debug, Clone, Default)]
pub struct TrackTags {
    pub title: String,
    /// The studio's own name, so a library of these files is recognisable.
    pub album: String,
    pub artist: String,
    pub genre: Option<String>,
    pub lyrics: Option<String>,
    pub bpm: Option<u32>,
    /// Cover image bytes with their media type, when the track has one.
    pub cover: Option<(String, Vec<u8>)>,
}

/// The formats the studio tags: the ones its tracks are kept in.
pub fn taggable(path: &Path) -> bool {
    matches!(FileType::from_path(path), Some(FileType::Mpeg | FileType::Flac | FileType::Wav))
}

/// Whether the file's own tag names the track. A FLAC always has a comment
/// block, holding at least the encoder's name, so a block alone is not a tag.
pub fn is_tagged(path: &Path) -> anyhow::Result<bool> {
    let file = Probe::open(path)?.options(ParseOptions::new().read_properties(false)).read()?;
    Ok(file.primary_tag().is_some_and(|tag| tag.title().is_some()))
}

/// The tempo field of a tag: ID3 keeps a whole number in TBPM, Vorbis comments a BPM.
fn bpm_key(tag_type: lofty::tag::TagType) -> ItemKey {
    if tag_type == lofty::tag::TagType::Id3v2 { ItemKey::IntegerBpm } else { ItemKey::Bpm }
}

/// The lyrics field of a tag: ID3's USLT frame, Vorbis comments' LYRICS.
fn lyrics_key(tag_type: lofty::tag::TagType) -> ItemKey {
    if tag_type == lofty::tag::TagType::Id3v2 { ItemKey::UnsyncLyrics } else { ItemKey::Lyrics }
}

/// Writes the tags onto an MP3, a FLAC or a WAV in place, replacing the tag that was there.
pub fn write_tags(path: &Path, tags: &TrackTags) -> anyhow::Result<()> {
    let file_type = FileType::from_path(path).ok_or_else(|| anyhow::anyhow!("{} is not an audio file the studio tags", path.display()))?;
    let mut tag = Tag::new(file_type.primary_tag_type());
    tag.set_title(tags.title.clone());
    if !tags.album.is_empty() {
        tag.set_album(tags.album.clone());
    }
    if !tags.artist.is_empty() {
        tag.set_artist(tags.artist.clone());
    }
    if let Some(genre) = tags.genre.as_deref().filter(|value| !value.trim().is_empty()) {
        tag.set_genre(genre.to_string());
    }
    if let Some(bpm) = tags.bpm {
        tag.insert_text(bpm_key(tag.tag_type()), bpm.to_string());
    }
    if let Some(lyrics) = tags.lyrics.as_deref().filter(|value| !value.trim().is_empty()) {
        tag.insert_text(lyrics_key(tag.tag_type()), lyrics.to_string());
    }
    if let Some((media_type, image)) = tags.cover.as_ref() {
        tag.push_picture(
            Picture::unchecked(image.clone())
                .pic_type(PictureType::CoverFront)
                .mime_type(MimeType::from_str(media_type))
                .description("Cover")
                .build(),
        );
    }
    tag.save_to_path(path, WriteOptions::default())?;
    if file_type == FileType::Wav {
        // RIFF INFO is what most readers of a WAV look at; the id3 chunk holds the lyrics and cover
        let mut info = tag;
        info.re_map(lofty::tag::TagType::RiffInfo);
        info.save_to_path(path, WriteOptions::default())?;
    }
    Ok(())
}


/// Chapter marks of a book, written after `write_tags` (which replaces the whole tag): ID3v2 CHAP frames with a
/// table of contents on an MP3, `CHAPTERnnn` / `CHAPTERnnnNAME` comments on a FLAC. Fewer than two chapters write none.
pub fn write_chapters(path: &Path, chapters: &[(f64, f64, String)]) -> anyhow::Result<()> {
    use lofty::file::AudioFile;
    use lofty::id3::v2::{ChapterFrame, ChapterTableOfContentsFrame, CtocFlags, Frame, FrameId, FrameList, TextInformationFrame};
    use lofty::TextEncoding;
    use std::borrow::Cow;
    if chapters.len() < 2 {
        return Ok(());
    }
    let ms = |s: f64| (s.max(0.0) * 1000.0).round() as u32;
    match FileType::from_path(path) {
        Some(FileType::Mpeg) => {
            let mut file = lofty::mpeg::MpegFile::read_from(&mut std::fs::File::open(path)?, ParseOptions::new())?;
            let tag = file.id3v2_mut().ok_or_else(|| anyhow::anyhow!("{} has no ID3v2 tag to hold its chapters", path.display()))?;
            let ids: Vec<Cow<'static, str>> = (0..chapters.len()).map(|i| Cow::Owned(format!("ch{i}"))).collect();
            for (id, (start, end, title)) in ids.iter().zip(chapters) {
                let mut children = FrameList::new();
                children.insert(Frame::Text(TextInformationFrame::new(FrameId::Valid(Cow::Borrowed("TIT2")), TextEncoding::UTF8, title.clone())));
                tag.insert(Frame::Chapter(ChapterFrame::new(id.clone(), ms(*start)..ms(*end), u32::MAX..u32::MAX, children)));
            }
            let toc = ChapterTableOfContentsFrame::new("toc", CtocFlags { top_level: true, ordered: true }, ids, FrameList::new());
            tag.insert(Frame::TableOfContents(toc));
            file.save_to_path(path, WriteOptions::default())?;
        }
        Some(FileType::Flac) => {
            let mut file = lofty::flac::FlacFile::read_from(&mut std::fs::File::open(path)?, ParseOptions::new())?;
            let comments = file.vorbis_comments_mut().ok_or_else(|| anyhow::anyhow!("{} has no comment block to hold its chapters", path.display()))?;
            for (i, (start, _, title)) in chapters.iter().enumerate() {
                let t = ms(*start);
                comments.push(format!("CHAPTER{:03}", i + 1), format!("{:02}:{:02}:{:02}.{:03}", t / 3_600_000, t / 60_000 % 60, t / 1000 % 60, t % 1000));
                comments.push(format!("CHAPTER{:03}NAME", i + 1), title.clone());
            }
            file.save_to_path(path, WriteOptions::default())?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(extension: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("studio-tag-{}.{extension}", uuid::Uuid::new_v4()));
        let silence = audio_post::Stereo { left: vec![0.0; 4800], right: vec![0.0; 4800], rate: 48_000 };
        match extension {
            "mp3" => std::fs::write(&path, audio_post::encode::mp3(&silence, 320).unwrap()).unwrap(),
            "flac" => std::fs::write(&path, audio_post::encode::flac(&silence).unwrap()).unwrap(),
            _ => crate::wavio::write_mono_f32(&path, &silence.left, 48_000).unwrap(),
        }
        path
    }

    #[test]
    fn a_written_tag_reads_back_from_every_kept_format() {
        for extension in ["mp3", "flac", "wav"] {
            let path = sample(extension);
            assert!(taggable(&path));
            assert!(!is_tagged(&path).unwrap());
            let tags = TrackTags {
                title: "Глава 1".into(),
                album: "Книга".into(),
                artist: "Автор".into(),
                lyrics: Some("[00:00.00]Привет".into()),
                ..TrackTags::default()
            };
            write_tags(&path, &tags).unwrap();
            assert!(is_tagged(&path).unwrap());
            let file = Probe::open(&path).unwrap().read().unwrap();
            let tag = file.primary_tag().unwrap();
            assert_eq!(tag.title().as_deref(), Some("Глава 1"));
            assert_eq!(tag.get_string(lyrics_key(tag.tag_type())), Some("[00:00.00]Привет"), "{extension}");
            let _ = std::fs::remove_file(&path);
        }
    }


    #[test]
    fn chapters_of_a_book_read_back_from_mp3_and_flac() {
        let chapters = vec![(0.0, 0.05, "Глава 1".to_string()), (0.05, 0.1, "Глава 2".to_string())];
        for extension in ["mp3", "flac"] {
            let path = sample(extension);
            write_tags(&path, &TrackTags { title: "Книга".into(), ..TrackTags::default() }).unwrap();
            write_chapters(&path, &chapters).unwrap();
            let file = Probe::open(&path).unwrap().read().unwrap();
            assert_eq!(file.primary_tag().unwrap().title().as_deref(), Some("Книга"), "{extension}: the tag survives");
            if extension == "mp3" {
                use lofty::file::AudioFile;
                let mpeg = lofty::mpeg::MpegFile::read_from(&mut std::fs::File::open(&path).unwrap(), ParseOptions::new()).unwrap();
                let titles: Vec<String> = mpeg
                    .id3v2()
                    .unwrap()
                    .iter()
                    .filter_map(|f| match f {
                        lofty::id3::v2::Frame::Chapter(c) => c.title().map(str::to_string),
                        _ => None,
                    })
                    .collect();
                assert_eq!(titles, ["Глава 1", "Глава 2"]);
            } else {
                use lofty::file::AudioFile;
                let flac = lofty::flac::FlacFile::read_from(&mut std::fs::File::open(&path).unwrap(), ParseOptions::new()).unwrap();
                let comments = flac.vorbis_comments().unwrap();
                assert_eq!(comments.get("CHAPTER002"), Some("00:00:00.050"));
                assert_eq!(comments.get("CHAPTER002NAME"), Some("Глава 2"));
            }
            let _ = std::fs::remove_file(&path);
        }
    }
}
