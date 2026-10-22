use std::fs;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use super::{library, prepare_reference, ref_pick::Word};

const NAME: &str = "Speaker";

fn voice(seconds: f64, sr: u32, channels: u16) -> (tempfile::TempDir, Vec<f32>) {
    let dir = tempfile::tempdir().unwrap();
    let samples: Vec<f32> = (0..(seconds * sr as f64) as usize)
        .map(|i| 0.4 * (i as f32 * std::f32::consts::TAU * 220.0 / sr as f32).sin())
        .collect();
    let spec = hound::WavSpec {
        channels,
        sample_rate: sr,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut out = hound::WavWriter::create(dir.path().join(format!("{NAME}.wav")), spec).unwrap();
    for &sample in &samples {
        if channels == 2 {
            out.write_sample(sample * 0.5).unwrap();
            out.write_sample(sample * 1.5).unwrap();
        } else {
            out.write_sample(sample).unwrap();
        }
    }
    out.finalize().unwrap();
    (dir, samples)
}

fn timed_voice(rotate: bool) -> (tempfile::TempDir, Vec<f32>) {
    let (dir, mut samples) = voice(14.0, 16_000, 1);
    let words = [
        Word {
            word: "First.".into(),
            start: 0.5,
            end: 2.5,
        },
        Word {
            word: "Best.".into(),
            start: 4.5,
            end: 7.5,
        },
        Word {
            word: "Last.".into(),
            start: 9.5,
            end: 12.0,
        },
    ];
    for (i, sample) in samples.iter_mut().enumerate() {
        let at = i as f64 / 16_000.0;
        if !words.iter().any(|w| w.start <= at && at < w.end) || (i / 1600) % 2 == 0 {
            *sample *= 0.0001;
        }
    }
    crate::audio::wavio::write_mono_f32(&dir.path().join(format!("{NAME}.wav")), &samples, 16_000).unwrap();
    library::write_words(dir.path(), NAME, &words).unwrap();
    library::write_transcript(dir.path(), NAME, "The full transcript must not be used for a fragment.").unwrap();
    library::write_card(
        dir.path(),
        NAME,
        &library::VoiceCard {
            rotate,
            ..Default::default()
        },
    )
    .unwrap();
    (dir, samples)
}

fn assert_format(path: &Path, sr: u32, seconds: f64) {
    let reader = hound::WavReader::open(path).unwrap();
    let spec = reader.spec();
    assert_eq!(spec.channels, 1);
    assert_eq!(spec.sample_rate, sr);
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(spec.sample_format, hound::SampleFormat::Int);
    assert!(
        (reader.duration() as f64 - seconds * sr as f64).abs() <= 1.0,
        "{}: {} frames",
        path.display(),
        reader.duration()
    );
    assert_eq!(&fs::read(path).unwrap()[20..22], &[1, 0], "the engine needs a simple PCM header");
}

fn assert_excerpt(path: &Path, source: &[f32], sr: u32, start: f64, end: f64) {
    assert_format(path, sr, end - start);
    let (actual, _) = crate::audio::wavio::read_mono_f32(path).unwrap();
    let offset = (start * sr as f64).round() as usize;
    let edge = sr as usize / 20;
    // The 15 ms fades intentionally change the edges of the reference.
    for i in (edge..actual.len() - edge).step_by(97) {
        assert!(
            (actual[i] - source[offset + i]).abs() < 0.0001,
            "different source sample at {i}, start {start}"
        );
    }
}

#[test]
fn short_stereo_reference_keeps_its_transcript_and_source_rate() {
    let (dir, source) = voice(1.0, 44_100, 2);
    library::write_transcript(dir.path(), NAME, "  Full reference.  ").unwrap();
    let (wav, text) = prepare_reference(dir.path(), NAME, 3.0, 1).unwrap();
    assert_eq!(text.as_deref(), Some("Full reference."));
    assert_excerpt(&wav, &source, 44_100, 0.0, 1.0);
    assert_eq!(fs::read_to_string(wav.with_extension("txt")).unwrap(), "Full reference.");
}

#[test]
fn high_rate_reference_stays_compatible_with_the_engine_wav_reader() {
    let (dir, _) = voice(1.0, 96_000, 1);
    let (wav, text) = prepare_reference(dir.path(), NAME, 3.0, 1).unwrap();
    assert!(text.is_none());
    assert_format(&wav, 48_000, 1.0);
}

#[test]
fn half_second_tolerance_keeps_the_complete_reference() {
    for (seconds, expected_seconds, expected_text) in [(2.5, 2.5, Some("Full reference.")), (2.6, 2.0, None)] {
        let (dir, source) = voice(seconds, 16_000, 1);
        library::write_transcript(dir.path(), NAME, "Full reference.").unwrap();
        let (wav, text) = prepare_reference(dir.path(), NAME, 2.0, 1).unwrap();
        assert_eq!(text.as_deref(), expected_text);
        assert_excerpt(&wav, &source, 16_000, 0.0, expected_seconds);
    }
}

#[test]
fn long_reference_without_usable_timings_uses_the_head_without_text() {
    for words in [
        None,
        Some(Vec::new()),
        Some(vec![Word {
            word: "Too short.".into(),
            start: 0.5,
            end: 0.6,
        }]),
    ] {
        let (dir, source) = voice(6.0, 16_000, 1);
        library::write_transcript(dir.path(), NAME, "A transcript of the entire long recording.").unwrap();
        if let Some(words) = words {
            library::write_words(dir.path(), NAME, &words).unwrap();
        }
        let (wav, text) = prepare_reference(dir.path(), NAME, 3.0, 1).unwrap();
        assert!(text.is_none());
        assert_excerpt(&wav, &source, 16_000, 0.0, 3.0);
        assert_eq!(fs::read_to_string(wav.with_extension("txt")).unwrap(), "");
    }
}

#[test]
fn timed_reference_selects_the_best_fragment_and_ignores_take_without_rotation() {
    let (dir, source) = timed_voice(false);
    let first = prepare_reference(dir.path(), NAME, 3.0, 1).unwrap();
    assert_eq!(first.1.as_deref(), Some("Best."));
    assert_excerpt(&first.0, &source, 16_000, 4.35, 7.7);
    assert_eq!(prepare_reference(dir.path(), NAME, 3.0, 9).unwrap(), first);
}

#[test]
fn rotation_uses_take_order_wraps_and_reuses_each_take_cache() {
    let (dir, source) = timed_voice(true);
    let mut paths = Vec::new();
    for (i, (text, start, end)) in [
        ("Best.", 4.35, 7.7),
        ("Last.", 9.35, 12.2),
        ("First.", 0.35, 2.7),
        ("Best.", 4.35, 7.7),
    ]
    .into_iter()
    .enumerate()
    {
        let take = i as u32 + 1;
        let prepared = prepare_reference(dir.path(), NAME, 3.0, take).unwrap();
        assert_eq!(prepared.1.as_deref(), Some(text));
        assert_excerpt(&prepared.0, &source, 16_000, start, end);
        let modified = fs::metadata(&prepared.0).unwrap().modified().unwrap();
        assert_eq!(prepare_reference(dir.path(), NAME, 3.0, take).unwrap(), prepared);
        assert_eq!(fs::metadata(&prepared.0).unwrap().modified().unwrap(), modified);
        assert!(!paths.contains(&prepared.0), "each take retains its own cache key");
        paths.push(prepared.0);
    }
    assert_eq!(prepare_reference(dir.path(), NAME, 3.0, 0).unwrap().0, paths[0]);
    assert!(fs::read_dir(dir.path().join(".work/refs"))
        .unwrap()
        .all(|e| !e.unwrap().file_name().to_string_lossy().ends_with("-16k.wav")));
}

#[test]
fn existing_cache_key_and_contents_are_reused_without_decoding_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join(format!("{NAME}.wav"));
    fs::write(&source, b"cached-source").unwrap();
    let stamp = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    fs::File::options()
        .write(true)
        .open(&source)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(stamp))
        .unwrap();
    let cache = dir.path().join(".work/refs");
    fs::create_dir_all(&cache).unwrap();
    let tag = blake3::hash(b"Speaker|13|1700000000|3.0|0||").to_hex()[..16].to_string();
    let wav = cache.join(format!("{tag}.wav"));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&wav, spec).unwrap();
    writer.write_sample(123_i16).unwrap();
    writer.finalize().unwrap();
    let txt = wav.with_extension("txt");
    fs::write(&txt, "  Cached fragment.\n").unwrap();
    let before = [fs::read(&wav).unwrap(), fs::read(&txt).unwrap()];
    let modified = [
        fs::metadata(&wav).unwrap().modified().unwrap(),
        fs::metadata(&txt).unwrap().modified().unwrap(),
    ];

    assert_eq!(
        prepare_reference(dir.path(), NAME, 3.0, 42).unwrap(),
        (wav.clone(), Some("  Cached fragment.\n".into()))
    );
    assert_eq!([fs::read(&wav).unwrap(), fs::read(&txt).unwrap()], before);
    assert_eq!(
        [
            fs::metadata(&wav).unwrap().modified().unwrap(),
            fs::metadata(&txt).unwrap().modified().unwrap()
        ],
        modified
    );
}

#[test]
fn missing_voice_returns_the_original_error_without_creating_a_cache() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        prepare_reference(dir.path(), "Missing", 3.0, 1).unwrap_err(),
        "no voice named Missing"
    );
    assert!(!dir.path().join(".work").exists());
}

#[test]
fn invalid_card_and_invalid_audio_errors_are_propagated() {
    let (dir, _) = voice(1.0, 16_000, 1);
    let card = dir.path().join(format!("{NAME}.json"));
    fs::write(&card, "{invalid").unwrap();
    assert!(prepare_reference(dir.path(), NAME, 3.0, 1)
        .unwrap_err()
        .contains(&card.display().to_string()));
    fs::remove_file(&card).unwrap();
    fs::write(dir.path().join(format!("{NAME}.wav")), b"invalid audio").unwrap();
    assert!(!prepare_reference(dir.path(), NAME, 3.0, 1).unwrap_err().is_empty());
    assert_eq!(fs::read_dir(dir.path().join(".work/refs")).unwrap().count(), 0);
}
