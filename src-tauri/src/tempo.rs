use aubio::{Onset, OnsetMode};
use serde::Serialize;

use crate::state::{issue, Result};

pub const RATE: usize = 44_100;
const HOP: usize = 256;
const PERIOD_STEP: usize = 10;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub bpm: Option<f64>,
    pub leading: f64,
    pub first_beat: Option<f64>,
}

pub fn wave_samples(wave: &[u8]) -> Result<&[u8]> {
    let invalid = "无法读取用于 BPM 分析的 PCM 音频";
    if wave.get(..4) != Some(b"RIFF") || wave.get(8..12) != Some(b"WAVE") {
        return Err(invalid.into());
    }
    let mut chunks = &wave[12..];
    while let Some(header) = chunks.get(..8) {
        let size = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        let data = &chunks[8..];
        if &header[..4] == b"data" {
            // FFmpeg writes an unknown data size when the WAV is sent through a pipe.
            return Ok(&data[..data.len().min(size)]);
        }
        chunks = data.get(size.saturating_add(size & 1)..).ok_or(invalid)?;
    }
    Err(invalid.into())
}

pub fn analyze(pcm: &[u8]) -> Result<Analysis> {
    if pcm.len() < 8 {
        return Err("音轨没有可分析的声音".into());
    }
    let mut peaks = Vec::new();
    let mut mono = Vec::with_capacity(pcm.len() / 8);
    for window in pcm.chunks(RATE / 100 * 8) {
        let mut peak = 0.0_f32;
        for frame in window.chunks_exact(8) {
            let left = f32::from_le_bytes(frame[..4].try_into().unwrap());
            let right = f32::from_le_bytes(frame[4..].try_into().unwrap());
            peak = peak.max(left.abs()).max(right.abs());
            mono.push((left + right) * 0.5);
        }
        peaks.push(peak);
    }

    // Keep -60 dBFS for normally mastered tracks and lower the gate for quiet recordings.
    let peak = peaks.iter().copied().fold(0.0_f32, f32::max);
    let gate = 0.001_f32
        .min(peak * 10.0_f32.powf(-42.0 / 20.0))
        .max(0.00001);
    let window = peaks.iter().position(|value| *value > gate);
    let leading = window.map_or(mono.len(), |index| {
        let start = index * (RATE / 100);
        let end = (start + RATE / 100).min(mono.len());
        (start..end)
            .find(|sample| {
                let frame = &pcm[sample * 8..sample * 8 + 8];
                frame
                    .chunks_exact(4)
                    .any(|channel| f32::from_le_bytes(channel.try_into().unwrap()).abs() > gate)
            })
            .unwrap_or(start)
    }) as f64
        / RATE as f64;

    let mut detector = Onset::new(OnsetMode::Complex, HOP * 4, HOP, RATE as u32)
        .map_err(|error| issue("无法初始化 BPM 分析", error))?;
    if peak > 0.0 && peak < 0.1 {
        // Normalize only the analysis signal so aubio's silence gate can hear quiet music.
        for sample in &mut mono {
            *sample *= 0.8 / peak;
        }
    }
    let mut hits = Vec::new();
    for block in mono.chunks_exact(HOP) {
        if detector
            .do_result(block)
            .map_err(|error| issue("无法分析起音", error))?
            > 0.0
        {
            let pos = detector.get_last();
            if pos < mono.len() {
                let strength = if hits.len() < 100 {
                    let local = &mono[pos.saturating_sub(100)..(pos + 100).min(mono.len())];
                    let average = local.iter().map(|x| x.abs()).sum::<f32>() / local.len() as f32;
                    if average == 0.0 {
                        // An isolated hit can be estimated before its wavefront.
                        let attack = &mono[pos..(pos + RATE / 40).min(mono.len())];
                        attack.iter().map(|x| x.abs()).sum::<f32>() as f64 / attack.len() as f64
                    } else {
                        average as f64
                    }
                } else {
                    1.0
                };
                hits.push((pos, strength));
            }
        }
    }
    let bpm = find_bpm(&hits);
    let first_beat = bpm
        .map(|bpm| {
            let period = 60.0 / bpm;
            let (_, phase) = PhaseTest::new(1, &hits).score(RATE as f64 * period, false, false);
            let phase = phase as f64 / RATE as f64;
            let offbeat = (phase + period * 0.5) % period;
            let phase =
                if slope_support(&mono, period, phase) >= slope_support(&mono, period, offbeat) {
                    phase
                } else {
                    offbeat
                };
            let first_on_grid = hits
                .iter()
                .map(|(pos, _)| *pos as f64 / RATE as f64)
                .find_map(|time| {
                    let grid = phase + ((time - phase) / period).round() * period;
                    (time >= leading - 0.025 && (grid - time).abs() <= 0.025)
                        .then_some(grid.max(leading))
                });
            first_on_grid.unwrap_or_else(|| phase + ((leading - phase) / period).ceil() * period)
        })
        .or_else(|| {
            hits.iter()
                .map(|(pos, _)| *pos as f64 / RATE as f64)
                .find(|time| *time >= leading - 0.025)
                .map(|time| time.max(leading))
        });
    Ok(Analysis {
        bpm,
        leading,
        first_beat,
    })
}

// ArrowVortex folds aubio onsets into each candidate beat period and scores the
// strongest beat phase with a Hamming window and half-weighted offbeats.
// Reference: uvcat7/ArrowVortex, src/Editor/FindTempo.cpp and FindOnsets.cpp.
struct PhaseTest<'a> {
    shift: usize,
    hits: &'a [(usize, f64)],
    oscillator: Vec<(f64, f64)>,
}

impl<'a> PhaseTest<'a> {
    fn new(shift: usize, hits: &'a [(usize, f64)]) -> Self {
        let width = 2048 >> shift;
        let oscillator = (0..width - 1)
            .map(|i| (std::f64::consts::TAU * i as f64 / (width - 1) as f64).sin_cos())
            .collect();
        Self {
            shift,
            hits,
            oscillator,
        }
    }

    fn score(&self, period: f64, reduce: bool, weighted: bool) -> (f64, usize) {
        let shift = if reduce { self.shift } else { 0 };
        let bins = period.round() as usize >> shift;
        let mut weights = vec![0.0; bins + 1];
        let mut positions = Vec::with_capacity(self.hits.len());
        for &(sample, strength) in self.hits {
            let pos = ((sample as f64 % period) as usize) >> shift;
            positions.push(pos);
            weights[pos] += if weighted { strength } else { 1.0 };
        }
        // Prefix sums of the histogram and its sine/cosine terms let us
        // evaluate the Hamming window at each candidate in constant time.
        let cycle = self.oscillator.len();
        let window_length = cycle + 1;
        let half = window_length / 2;
        let mut prefix = Vec::with_capacity(bins + 1);
        let mut sum = [0.0; 3];
        prefix.push(sum);
        for (i, &weight) in weights[..bins].iter().enumerate() {
            let (sin, cos) = self.oscillator[i % cycle];
            sum[0] += weight;
            sum[1] += weight * sin;
            sum[2] += weight * cos;
            prefix.push(sum);
        }
        let area = |from: usize, to: usize, window_start: usize| -> f64 {
            let offset =
                (window_start as isize - from as isize).rem_euclid(cycle as isize) as usize;
            let (sin, cos) = self.oscillator[offset];
            let [total, sine, cosine] = std::array::from_fn(|i| prefix[to][i] - prefix[from][i]);
            0.54 * total - 0.46 * (cosine * cos - sine * sin)
        };
        let confidence = |pos: usize| -> f64 {
            let start = pos.saturating_sub(half);
            let end = (pos + half).min(bins);
            let left = if pos < half {
                area(bins + pos - half, bins, 0)
            } else {
                0.0
            };
            let right = if pos + half > bins {
                area(0, pos + half - bins, window_length - (pos + half - bins))
            } else {
                0.0
            };
            left + right + area(start, end, 0)
        };
        let mut best = (0.0, 0);
        for pos in positions {
            let score = confidence(pos) + confidence((pos + bins / 2) % bins) * 0.5;
            if score > best.0 {
                best = (score, pos);
            }
        }
        best
    }
}

fn find_bpm(hits: &[(usize, f64)]) -> Option<f64> {
    if hits.len() < 2 {
        return None;
    }
    let scale = RATE as f64 * 60.0;
    let min = (scale / 205.0).round() as usize;
    let max = (scale / 89.0).round() as usize;
    let coarse = PhaseTest::new(3, hits);
    let fine = PhaseTest::new(0, hits);
    let mut fitness = vec![None; max - min];
    let coarse_scores: Vec<_> = (min..max)
        .step_by(PERIOD_STEP)
        .map(|p| (p, coarse.score(p as f64, true, true).0.max(0.001)))
        .collect();
    let coordinate = |p: f64| (p - min as f64) / (max - min) as f64;
    let coefficients = cubic_fit(
        coarse_scores
            .iter()
            .map(|&(p, score)| (coordinate(p as f64), score)),
    );
    let baseline = |p: f64| {
        coefficients
            .iter()
            .rev()
            .fold(0.0, |sum, &coef| sum * coordinate(p) + coef)
    };
    for &(p, score) in &coarse_scores {
        fitness[p - min] = Some(score - baseline(p as f64));
    }
    let threshold = fitness.iter().flatten().copied().fold(0.001, f64::max) * 0.4;
    let mut candidates = Vec::new();
    for &(p, score) in &coarse_scores {
        if score - baseline(p as f64) <= threshold {
            continue;
        }
        let mut best = (p, 0.0);
        for candidate in p.saturating_sub(PERIOD_STEP).max(min)..(p + PERIOD_STEP).min(max) {
            let score = *fitness[candidate - min].get_or_insert_with(|| {
                (coarse.score(candidate as f64, true, true).0 - baseline(candidate as f64)).max(0.1)
            });
            if score > best.1 {
                best = (candidate, score);
            }
        }
        candidates.push((scale / best.0 as f64, best.1));
    }
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut distinct: Vec<(f64, f64)> = Vec::new();
    for (bpm, score) in candidates {
        if !distinct
            .iter()
            .any(|&(x, _)| [x, x * 2.0, x * 0.5].iter().any(|x| (x - bpm).abs() < 0.1))
        {
            distinct.push((bpm, score));
        }
    }
    let fine_score = |bpm: f64| {
        let period = scale / bpm;
        fine.score(period, false, true).0 - baseline(period)
    };
    for (bpm, _) in &mut distinct {
        let rounded = bpm.round();
        let difference = (*bpm - rounded).abs();
        if difference < 0.01 || (difference < 0.05 && fine_score(rounded) > fine_score(*bpm) * 0.99)
        {
            *bpm = rounded;
        }
    }
    if distinct.len() >= 2 && distinct[0].1 / distinct[1].1 < 1.05 {
        for (bpm, score) in &mut distinct {
            *score = fine_score(*bpm);
        }
        distinct.sort_by(|a, b| b.1.total_cmp(&a.1));
    }
    distinct
        .first()
        .map(|&(bpm, _)| (bpm * 1000.0).round() / 1000.0)
}

fn cubic_fit(points: impl Iterator<Item = (f64, f64)>) -> [f64; 4] {
    let mut matrix = [[0.0; 5]; 4];
    for (x, y) in points {
        let powers = [1.0, x, x * x, x * x * x];
        for (row, &power) in matrix.iter_mut().zip(&powers) {
            for (cell, &coefficient) in row.iter_mut().zip(&powers) {
                *cell += power * coefficient;
            }
            row[4] += power * y;
        }
    }
    for i in 0..4 {
        let pivot = (i..4)
            .max_by(|&a, &b| matrix[a][i].abs().total_cmp(&matrix[b][i].abs()))
            .unwrap();
        matrix.swap(i, pivot);
        let divisor = matrix[i][i];
        for entry in &mut matrix[i] {
            *entry /= divisor;
        }
        let row = matrix[i];
        for (j, other) in matrix.iter_mut().enumerate() {
            if i == j {
                continue;
            }
            let coefficient = other[i];
            for (entry, value) in other.iter_mut().zip(row) {
                *entry -= coefficient * value;
            }
        }
    }
    matrix.map(|row| row[4])
}

fn slope_support(samples: &[f32], period: f64, phase: f64) -> f64 {
    let half = RATE / 20;
    let mut pos = phase * RATE as f64;
    let mut total = 0.0;
    while pos < samples.len() as f64 {
        let center = pos as usize;
        if center >= half && center + half <= samples.len() {
            let amplitude = |start, end| {
                samples[start..end]
                    .iter()
                    .map(|x| x.abs() as f64)
                    .sum::<f64>()
            };
            total += (amplitude(center, center + half) - amplitude(center - half, center)).max(0.0);
        }
        pos += period * RATE as f64;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzes_fractional_bpm_and_quiet_intro() {
        let bpm = 123.45;
        let start = 0.713;
        let mut pcm = Vec::new();
        for frame in 0..RATE * 22 {
            let time = frame as f64 / RATE as f64 - start;
            let beat = time.rem_euclid(60.0 / bpm);
            let sample = if time >= 0.0 && beat < 0.060 {
                (0.00002 + (std::f64::consts::TAU * 78.0 * beat).sin() * 0.00004) as f32
            } else {
                0.0
            };
            let bytes = sample.to_le_bytes();
            pcm.extend(bytes);
            pcm.extend(bytes);
        }
        let result = analyze(&pcm).unwrap();
        assert!((result.bpm.unwrap() - bpm).abs() < 0.15, "{:?}", result.bpm);
        assert!((result.leading - start).abs() < 0.001);
        assert!((result.first_beat.unwrap() - start).abs() < 0.030);
    }

    #[test]
    fn does_not_invent_bpm_for_silence() {
        let analysis = analyze(&vec![0; RATE * 8]).unwrap();
        assert!(analysis.bpm.is_none());
        assert_eq!(analysis.leading, 1.0);
        assert!(analysis.first_beat.is_none());
    }
}
