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
}
