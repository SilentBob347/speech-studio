//! Тишина в клипе TTS до подгонки темпа: края срезаются по порогу от пика клипа, длинные паузы внутри
//! сжимаются, только когда клип без ускорения в свой слот не влезает. Чистые функции над моно-сэмплами.
//!
//! Громкость — RMS по кадрам 20 мс; порог тишины — пик покадрового RMS минус 40 дБ, но не ниже −70 dBFS.
//! Все сдвиги кратны кадру: повторный прогон по уже обрезанному клипу видит ту же сетку кадров, тот же
//! пик и те же громкие кадры, поэтому ничего не меняет.

const FRAME_SECS: f64 = 0.020;
const THRESHOLD_BELOW_PEAK_DB: f64 = 40.0;
const THRESHOLD_FLOOR_DBFS: f64 = -70.0;
/// Запас за крайним громким кадром: 3 кадра (60 мс) держат тихую конечную согласную клона. Фейд короче
/// запаса и ложится только на тихие кадры.
const EDGE_MARGIN_FRAMES: usize = 3;
const EDGE_FADE_SECS: f64 = 0.030;
/// Пауза длиннее 20 тихих кадров (0.4 с) становится 10 кадрами (0.2 с) со стыком в 1 кадр (20 мс).
const LONG_PAUSE_FRAMES: usize = 20;
const SHORT_PAUSE_FRAMES: usize = 10;
const PAUSE_XFADE_FRAMES: usize = 1;

/// Клип после подготовки и сколько сэмплов снято.
pub struct Tightened {
    pub samples: Vec<f32>,
    pub edge_cut: usize,
    pub pause_cut: usize,
}

/// Снять тишину по краям; паузы сжать, только если клип и после этого длиннее `room_secs` (длительности,
/// выше которой его пришлось бы ускорять).
pub fn tighten(x: &[f32], sr: u32, room_secs: f64) -> Tightened {
    let (edged, edge_cut) = trim_edges(x, sr);
    if edged.len() as f64 <= room_secs * sr as f64 {
        return Tightened { samples: edged, edge_cut, pause_cut: 0 };
    }
    let (samples, pause_cut) = compress_pauses(&edged, sr);
    Tightened { samples, edge_cut, pause_cut }
}

struct Level {
    hop: usize,
    loud: Vec<bool>,
}

fn frame_len(sr: u32) -> usize {
    (sr as f64 * FRAME_SECS).round() as usize
}

fn db_to_amp(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Громкие кадры клипа. None — кадров выше порога нет (тишина или шум ниже −70 dBFS): такой клип не
/// режется, его брак ловят проверки синтеза.
fn level(x: &[f32], sr: u32) -> Option<Level> {
    let hop = frame_len(sr);
    if hop == 0 || x.is_empty() {
        return None;
    }
    let rms: Vec<f64> = x
        .chunks(hop)
        .map(|c| (c.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / c.len() as f64).sqrt())
        .collect();
    let peak = rms.iter().copied().fold(0.0f64, f64::max);
    let thr = (peak * db_to_amp(-THRESHOLD_BELOW_PEAK_DB)).max(db_to_amp(THRESHOLD_FLOOR_DBFS));
    let loud: Vec<bool> = rms.iter().map(|&r| r > thr).collect();
    loud.contains(&true).then_some(Level { hop, loud })
}

fn raised_cos(t: f64) -> f64 {
    0.5 - 0.5 * (std::f64::consts::PI * t).cos()
}

fn fade_in(y: &mut [f32], n: usize) {
    let n = n.min(y.len());
    for (k, v) in y.iter_mut().take(n).enumerate() {
        *v *= raised_cos(k as f64 / n as f64) as f32;
    }
}

fn fade_out(y: &mut [f32], n: usize) {
    let n = n.min(y.len());
    for (k, v) in y.iter_mut().rev().take(n).enumerate() {
        *v *= raised_cos(k as f64 / n as f64) as f32;
    }
}

/// Срезать тишину до первого и после последнего громкого кадра, оставив запас; фейд только на
/// срезанном краю. Возвращает клип и число снятых сэмплов.
pub fn trim_edges(x: &[f32], sr: u32) -> (Vec<f32>, usize) {
    let Some((a, b)) = edge_bounds(x, sr) else {
        return (x.to_vec(), 0);
    };
    let mut y = x[a..b].to_vec();
    let fade = (sr as f64 * EDGE_FADE_SECS).round() as usize;
    if a > 0 {
        fade_in(&mut y, fade);
    }
    if b < x.len() {
        fade_out(&mut y, fade);
    }
    (y, x.len() - (b - a))
}

/// Где кончается тишина в начале и начинается в конце клипа, с запасом.
fn edge_bounds(x: &[f32], sr: u32) -> Option<(usize, usize)> {
    let lv = level(x, sr)?;
    let (first, last) = (lv.loud.iter().position(|&l| l)?, lv.loud.iter().rposition(|&l| l)?);
    Some((first.saturating_sub(EDGE_MARGIN_FRAMES) * lv.hop, ((last + 1 + EDGE_MARGIN_FRAMES) * lv.hop).min(x.len())))
}

/// Сжать паузы между громкими кадрами длиннее LONG_PAUSE_FRAMES до SHORT_PAUSE_FRAMES: от паузы остаются
/// её начало и конец, сшитые косинусным кроссфейдом. Возвращает клип и число снятых сэмплов.
pub fn compress_pauses(x: &[f32], sr: u32) -> (Vec<f32>, usize) {
    let (y, cuts) = squeeze_pauses(x, sr, LONG_PAUSE_FRAMES, SHORT_PAUSE_FRAMES);
    (y, cuts.iter().map(|c| c.1).sum())
}

/// Пауза дубля длиннее 60 кадров (1,2 с) — сбой движка, а не замысел: она становится 30 кадрами (0,6 с).
const ANOMALOUS_PAUSE_FRAMES: usize = 60;
const CAPPED_PAUSE_FRAMES: usize = 30;

/// Дубль, приготовленный к склейке, и где из него что снято: (место в исходном дубле, сколько сэмплов).
pub struct Tidied {
    pub samples: Vec<f32>,
    cuts: Vec<(usize, usize)>,
    sr: u32,
}

impl Tidied {
    /// Момент исходного дубля (с) — в приготовленном: время слова, распознанного в исходном.
    pub fn at(&self, secs: f64) -> f64 {
        let raw = (secs.max(0.0) * self.sr as f64) as usize;
        let removed: usize = self.cuts.iter().filter(|(p, _)| *p <= raw).map(|(p, r)| (*r).min(raw - p)).sum();
        (raw - removed) as f64 / self.sr as f64
    }
}

/// Дубль для склейки экспорта: без тишины по краям (паузы между блоками — ровно заданные) и без аномально длинных
/// пауз внутри.
pub fn tidy(x: &[f32], sr: u32) -> Tidied {
    let lead = edge_bounds(x, sr).map_or(0, |(a, _)| a);
    let (edged, _) = trim_edges(x, sr);
    let (samples, pauses) = squeeze_pauses(&edged, sr, ANOMALOUS_PAUSE_FRAMES, CAPPED_PAUSE_FRAMES);
    let mut cuts = vec![(0, lead)];
    cuts.extend(pauses.into_iter().map(|(p, r)| (p + lead, r)));
    Tidied { samples, cuts, sr }
}

/// What `tidy` does to a finished take, done to a take heard as it is made: a pause inside it keeps at most
/// `CAPPED_PAUSE_FRAMES`, its other quiet frames are not sent on.
#[derive(Default)]
pub struct PauseGate {
    peak: f64,
    quiet: usize,
}

impl PauseGate {
    pub fn pass(&mut self, x: &[f32], sr: u32) -> Vec<f32> {
        let hop = frame_len(sr).max(1);
        let mut out = Vec::with_capacity(x.len());
        for frame in x.chunks(hop) {
            let rms = (frame.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / frame.len() as f64).sqrt();
            self.peak = self.peak.max(rms);
            let thr = (self.peak * db_to_amp(-THRESHOLD_BELOW_PEAK_DB)).max(db_to_amp(THRESHOLD_FLOOR_DBFS));
            self.quiet = if rms > thr { 0 } else { self.quiet + 1 };
            if self.quiet <= CAPPED_PAUSE_FRAMES {
                out.extend_from_slice(frame);
            }
        }
        out
    }
}

/// Склеить `next` с концом `out` равномощным кроссфейдом в 30 мс: стык двух кусков одной фразы без паузы и щелчка.
pub fn crossfade_join(out: &mut Vec<f32>, next: &[f32], sr: u32) {
    let ov = ((sr as f64 * 0.030) as usize).min(out.len()).min(next.len());
    let start = out.len() - ov;
    for k in 0..ov {
        let g = (k as f64 + 0.5) / ov as f64 * std::f64::consts::FRAC_PI_2;
        out[start + k] = (out[start + k] as f64 * g.cos() + next[k] as f64 * g.sin()) as f32;
    }
    out.extend_from_slice(&next[ov..]);
}

fn squeeze_pauses(x: &[f32], sr: u32, long_frames: usize, short_frames: usize) -> (Vec<f32>, Vec<(usize, usize)>) {
    let Some(lv) = level(x, sr) else {
        return (x.to_vec(), Vec::new());
    };
    let (Some(first), Some(last)) = (lv.loud.iter().position(|&l| l), lv.loud.iter().rposition(|&l| l)) else {
        return (x.to_vec(), Vec::new());
    };
    let hop = lv.hop;
    let keep = short_frames * hop;
    let xf = PAUSE_XFADE_FRAMES * hop;
    let head = (keep + xf).div_ceil(2);
    let tail = keep + xf - head;
    let mut out = Vec::with_capacity(x.len());
    let mut pos = 0usize;
    let mut cuts: Vec<(usize, usize)> = Vec::new();
    let mut i = first;
    while i <= last {
        if lv.loud[i] {
            i += 1;
            continue;
        }
        let mut j = i;
        while !lv.loud[j] {
            j += 1;
        }
        if j - i > long_frames {
            let (p0, p1) = (i * hop, j * hop);
            out.extend_from_slice(&x[pos..p0 + head - xf]);
            for k in 0..xf {
                let g = raised_cos((k as f64 + 0.5) / xf as f64);
                let v = x[p0 + head - xf + k] as f64 * (1.0 - g) + x[p1 - tail + k] as f64 * g;
                out.push(v as f32);
            }
            pos = p1 - tail + xf;
            cuts.push((p0 + head - xf, (p1 - p0) - keep));
        }
        i = j;
    }
    out.extend_from_slice(&x[pos..]);
    (out, cuts)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_pause_heard_as_it_is_made_keeps_at_most_the_capped_length() {
        let sr = 24_000u32;
        let tone = |secs: f64| (0..(secs * sr as f64) as usize).map(|i| (i as f32 * 0.05).sin() * 0.5).collect::<Vec<f32>>();
        let mut gate = super::PauseGate::default();
        let mut heard = Vec::new();
        for piece in [tone(1.0), vec![0.0; 2 * sr as usize], tone(1.0)] {
            for chunk in piece.chunks(4_800) {
                heard.extend(gate.pass(chunk, sr));
            }
        }
        let secs = heard.len() as f64 / sr as f64;
        assert!((secs - 2.6).abs() < 0.05, "{secs}");
    }


    #[test]
    fn tidy_keeps_a_short_pause_and_caps_an_anomalous_one() {
        let sr = 24_000u32;
        let tone = |secs: f64| (0..(secs * sr as f64) as usize).map(|i| 0.5 * (i as f32 * 0.05).sin()).collect::<Vec<f32>>();
        let quiet = |secs: f64| vec![0.0f32; (secs * sr as f64) as usize];
        let mut x = quiet(0.5);
        x.extend(tone(0.5));
        x.extend(quiet(0.3));
        x.extend(tone(0.5));
        x.extend(quiet(2.0));
        x.extend(tone(0.5));
        x.extend(quiet(0.5));
        let y = tidy(&x, sr);
        let secs = y.samples.len() as f64 / sr as f64;
        assert!(secs > 1.5 + 0.3 + 0.55 && secs < 1.5 + 0.3 + 0.75, "{secs}");
        let third = y.at(0.5 + 0.5 + 0.3 + 0.5 + 2.0 + 0.1);
        assert!((third - (y.samples.len() as f64 / sr as f64 - 0.06 - 0.4)).abs() < 0.03, "{third}");
        assert!(y.at(0.6) < 0.2);
    }

    #[test]
    fn a_crossfade_joins_without_a_gap() {
        let mut a = vec![1.0f32; 2400];
        let b = vec![1.0f32; 2400];
        crossfade_join(&mut a, &b, 24_000);
        assert_eq!(a.len(), 2400 + 2400 - 720);
        assert!(a.iter().all(|v| *v > 0.99 && *v < 1.42));
    }

    use super::*;

    const SR: u32 = 24_000;

    fn secs(s: f64) -> usize {
        (s * SR as f64).round() as usize
    }

    fn silence(s: f64) -> Vec<f32> {
        vec![0.0; secs(s)]
    }

    fn tone(s: f64, amp: f32) -> Vec<f32> {
        (0..secs(s))
            .map(|i| amp * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / SR as f64).sin() as f32)
            .collect()
    }

    fn cat(parts: &[Vec<f32>]) -> Vec<f32> {
        parts.concat()
    }

    #[test]
    fn leading_and_trailing_silence_go_with_a_60ms_margin() {
        let body = tone(1.0, 0.5);
        let x = cat(&[silence(0.5), body.clone(), silence(0.8)]);
        let t = tighten(&x, SR, 100.0);
        assert_eq!(t.samples.len(), secs(1.12), "1 с речи и по 60 мс запаса с каждой стороны");
        assert_eq!(t.edge_cut, secs(1.18));
        assert_eq!(t.pause_cut, 0);
        assert_eq!(&t.samples[secs(0.06)..secs(1.06)], &body[..], "речь не тронута");
        assert_eq!(t.samples[0], 0.0);
        assert_eq!(*t.samples.last().unwrap(), 0.0);
    }

    #[test]
    fn quiet_final_consonant_below_threshold_survives() {
        let body = tone(1.0, 0.5);
        let consonant = tone(0.03, db_to_amp(-55.0) as f32);
        let x = cat(&[silence(0.5), body, consonant.clone(), silence(0.8)]);
        let (y, _) = trim_edges(&x, SR);
        assert_eq!(&y[secs(1.06)..secs(1.09)], &consonant[..], "согласная −55 dBFS при пике −6 цела");
        assert_eq!(y.len(), secs(1.12));
    }

    #[test]
    fn long_inner_pause_shrinks_only_when_the_clip_does_not_fit() {
        let second = tone(1.0, 0.5);
        let x = cat(&[tone(1.0, 0.5), silence(0.9), second.clone()]);
        let fits = tighten(&x, SR, 3.0);
        assert_eq!(fits.samples, x, "места хватает — естественная пауза остаётся");
        let tight = tighten(&x, SR, 2.0);
        assert_eq!(tight.samples.len(), secs(2.2), "пауза 0.9 с стала 0.2 с");
        assert_eq!(tight.pause_cut, secs(0.7));
        assert_eq!(&tight.samples[secs(1.2)..], &second[..], "вторая фраза цела");
        let gap = &tight.samples[secs(1.0)..secs(1.2)];
        assert!(gap.iter().all(|v| v.abs() < 1e-6), "на месте паузы тишина");
    }

    #[test]
    fn short_inner_pause_is_kept_even_when_the_clip_does_not_fit() {
        let x = cat(&[tone(1.0, 0.5), silence(0.3), tone(1.0, 0.5)]);
        let t = tighten(&x, SR, 1.0);
        assert_eq!(t.samples, x);
    }

    #[test]
    fn second_pass_over_a_trimmed_clip_changes_nothing() {
        let consonant = tone(0.03, db_to_amp(-55.0) as f32);
        let x = cat(&[silence(0.5), tone(1.0, 0.5), silence(0.9), tone(0.7, 0.3), consonant, silence(0.8)]);
        for room in [100.0, 1.5] {
            let once = tighten(&x, SR, room);
            assert!(once.edge_cut > 0);
            let twice = tighten(&once.samples, SR, room);
            assert_eq!(twice.samples, once.samples, "room={room}");
            assert_eq!((twice.edge_cut, twice.pause_cut), (0, 0));
        }
        let unaligned = cat(&[silence(0.4567), tone(0.8, 0.4), silence(0.3333)]);
        let once = tighten(&unaligned, SR, 100.0);
        assert_eq!(tighten(&once.samples, SR, 100.0).samples, once.samples);
    }

    #[test]
    fn silent_clip_is_left_as_is() {
        let zeros = silence(1.0);
        let t = tighten(&zeros, SR, 0.1);
        assert_eq!(t.samples, zeros);
        assert_eq!((t.edge_cut, t.pause_cut), (0, 0));
        let hiss = tone(1.0, db_to_amp(-80.0) as f32);
        assert_eq!(tighten(&hiss, SR, 0.1).samples, hiss, "шум ниже −70 dBFS — не речь");
        assert!(tighten(&[], SR, 0.1).samples.is_empty());
    }

    #[test]
    fn quiet_clone_is_trimmed_against_its_own_peak() {
        let quiet = tone(1.0, db_to_amp(-50.0) as f32);
        let x = cat(&[silence(0.5), quiet, silence(0.5)]);
        let (y, cut) = trim_edges(&x, SR);
        assert_eq!(y.len(), secs(1.12));
        assert_eq!(cut, secs(0.88));
    }
}
