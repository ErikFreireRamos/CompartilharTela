//! Codecs de áudio estéreo em Rust puro (IMA-ADPCM 4:1 e PCM 16-bit) + resampler linear.

const INDEX_TABLE: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];

const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408,
    449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066,
    2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630,
    9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794,
    32767,
];

#[derive(Clone, Copy, Default)]
struct ChannelState {
    predictor: i32,
    index: usize,
}

impl ChannelState {
    #[inline]
    fn encode_sample(&mut self, sample: i16) -> u8 {
        let step = STEP_TABLE[self.index];
        let mut diff = i32::from(sample) - self.predictor;
        let mut nibble = 0u8;
        if diff < 0 {
            nibble = 8;
            diff = -diff;
        }
        let mut delta = step >> 3;
        if diff >= step {
            nibble |= 4;
            diff -= step;
            delta += step;
        }
        if diff >= (step >> 1) {
            nibble |= 2;
            diff -= step >> 1;
            delta += step >> 1;
        }
        if diff >= (step >> 2) {
            nibble |= 1;
            delta += step >> 2;
        }
        if (nibble & 8) != 0 {
            self.predictor = (self.predictor - delta).clamp(-32768, 32767);
        } else {
            self.predictor = (self.predictor + delta).clamp(-32768, 32767);
        }
        let next_idx = (self.index as i32 + INDEX_TABLE[nibble as usize]).clamp(0, 88);
        self.index = next_idx as usize;
        nibble
    }

    #[inline]
    fn decode_nibble(&mut self, nibble: u8) -> i16 {
        let step = STEP_TABLE[self.index];
        let mut delta = step >> 3;
        if (nibble & 4) != 0 {
            delta += step;
        }
        if (nibble & 2) != 0 {
            delta += step >> 1;
        }
        if (nibble & 1) != 0 {
            delta += step >> 2;
        }
        if (nibble & 8) != 0 {
            self.predictor = (self.predictor - delta).clamp(-32768, 32767);
        } else {
            self.predictor = (self.predictor + delta).clamp(-32768, 32767);
        }
        let next_idx = (self.index as i32 + INDEX_TABLE[nibble as usize]).clamp(0, 88);
        self.index = next_idx as usize;
        self.predictor as i16
    }
}

/// Reamostra áudio estéreo intercalado `f32` de `src_rate` para `dst_rate` (interpolação linear).
pub fn resample_stereo_f32(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate || samples.len() < 2 {
        return samples.to_vec();
    }
    let src_frames = samples.len() / 2;
    let dst_frames = ((src_frames as u64 * u64::from(dst_rate)) / u64::from(src_rate.max(1))) as usize;
    if dst_frames == 0 {
        return Vec::new();
    }
    let ratio = src_frames as f64 / dst_frames as f64;
    let mut out = Vec::with_capacity(dst_frames * 2);
    for i in 0..dst_frames {
        let pos = i as f64 * ratio;
        let idx0 = (pos.floor() as usize).min(src_frames - 1);
        let idx1 = (idx0 + 1).min(src_frames - 1);
        let frac = (pos - idx0 as f64) as f32;
        let l = samples[idx0 * 2] * (1.0 - frac) + samples[idx1 * 2] * frac;
        let r = samples[idx0 * 2 + 1] * (1.0 - frac) + samples[idx1 * 2 + 1] * frac;
        out.push(l);
        out.push(r);
    }
    out
}

/// Codifica amostras estéreo intercaladas `f32` em IMA-ADPCM 4:1.
pub fn encode_stereo_f32(samples: &[f32]) -> Vec<u8> {
    let frames = samples.len() / 2;
    if frames == 0 {
        return Vec::new();
    }
    let mut left = ChannelState {
        predictor: (samples[0].clamp(-1.0, 1.0) * 32767.0) as i32,
        index: 0,
    };
    let mut right = ChannelState {
        predictor: (samples[1].clamp(-1.0, 1.0) * 32767.0) as i32,
        index: 0,
    };

    let mut out = Vec::with_capacity(8 + frames.saturating_sub(1));
    out.extend_from_slice(&(left.predictor as i16).to_le_bytes());
    out.push(left.index as u8);
    out.extend_from_slice(&(right.predictor as i16).to_le_bytes());
    out.push(right.index as u8);
    out.extend_from_slice(&(frames as u16).to_le_bytes());

    for i in 1..frames {
        let l = (samples[i * 2].clamp(-1.0, 1.0) * 32767.0) as i16;
        let r = (samples[i * 2 + 1].clamp(-1.0, 1.0) * 32767.0) as i16;
        let ln = left.encode_sample(l);
        let rn = right.encode_sample(r);
        out.push((ln & 0x0f) | ((rn & 0x0f) << 4));
    }
    out
}

/// Decodifica um bloco IMA-ADPCM estéreo de volta para `f32` intercalado.
pub fn decode_stereo_f32(data: &[u8]) -> Option<Vec<f32>> {
    if data.len() < 8 {
        return None;
    }
    let mut left = ChannelState {
        predictor: i32::from(i16::from_le_bytes([data[0], data[1]])),
        index: usize::from(data[2].min(88)),
    };
    let mut right = ChannelState {
        predictor: i32::from(i16::from_le_bytes([data[3], data[4]])),
        index: usize::from(data[5].min(88)),
    };
    let frames = u16::from_le_bytes([data[6], data[7]]) as usize;
    if frames == 0 || data.len() < 8 + frames.saturating_sub(1) {
        return None;
    }

    let mut out = Vec::with_capacity(frames * 2);
    out.push(left.predictor as f32 / 32768.0);
    out.push(right.predictor as f32 / 32768.0);

    for &byte in &data[8..8 + frames - 1] {
        let l = left.decode_nibble(byte & 0x0f);
        let r = right.decode_nibble((byte >> 4) & 0x0f);
        out.push(f32::from(l) / 32768.0);
        out.push(f32::from(r) / 32768.0);
    }
    Some(out)
}

/// Codifica áudio estéreo `f32` em PCM 16-bit sem perdas (qualidade Estúdio).
pub fn encode_pcm16_stereo(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Decodifica PCM 16-bit estéreo de volta para `f32`.
pub fn decode_pcm16_stereo(data: &[u8]) -> Option<Vec<f32>> {
    if data.len() < 4 || !data.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(data.len() / 2);
    for chunk in data.chunks_exact(2) {
        let v = i16::from_le_bytes([chunk[0], chunk[1]]);
        out.push(f32::from(v) / 32768.0);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_sine_wave_stereo() {
        let mut input = Vec::with_capacity(960 * 2);
        for i in 0..960 {
            let t = i as f32 / 48_000.0;
            input.push((t * 440.0 * std::f32::consts::TAU).sin() * 0.5);
            input.push((t * 660.0 * std::f32::consts::TAU).sin() * 0.4);
        }
        let encoded = encode_stereo_f32(&input);
        let decoded = decode_stereo_f32(&encoded).unwrap();
        assert_eq!(decoded.len(), input.len());

        let mse: f32 = input
            .iter()
            .zip(decoded.iter())
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f32>()
            / input.len() as f32;
        assert!(mse < 0.002, "MSE too high: {mse}");
    }

    #[test]
    fn resample_48k_to_32k_and_back() {
        let input = vec![0.5f32; 960 * 2];
        let down = resample_stereo_f32(&input, 48_000, 32_000);
        assert_eq!(down.len(), 640 * 2);
        let up = resample_stereo_f32(&down, 32_000, 48_000);
        assert_eq!(up.len(), 960 * 2);
    }
}
