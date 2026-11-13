//! 再生の音（ROADMAP M6.175）。OS の既定の出力デバイス（cpal: Windows は WASAPI、macOS は Core Audio、Linux は
//! ALSA）に、engine の [`AudioFeed`] から音を渡す。デバイスがない・開けないときは、音なしで映像だけを再生する。

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use disccraft_engine::AudioFeed;

/// 開いた出力デバイス（まだ鳴らしていない）。
pub(crate) struct AudioDevice {
    device: cpal::Device,
    config: StreamConfig,
    format: SampleFormat,
}

/// 鳴っている出力。落とすと止まる。
pub(crate) struct AudioOut {
    _stream: cpal::Stream,
}

impl AudioDevice {
    /// 既定の出力デバイスとその既定の形式。
    pub(crate) fn open_default() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no audio output device".to_string())?;
        let supported = device
            .default_output_config()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            device,
            format: supported.sample_format(),
            config: supported.into(),
        })
    }

    /// サンプリング周波数（Hz）。音はこの周波数でデコードする。
    pub(crate) fn rate(&self) -> u32 {
        self.config.sample_rate
    }

    /// `feed` の音を鳴らし始める。
    pub(crate) fn play(self, feed: AudioFeed) -> Result<AudioOut, String> {
        let stream = match self.format {
            SampleFormat::F32 => self.build::<f32>(feed),
            SampleFormat::F64 => self.build::<f64>(feed),
            SampleFormat::I16 => self.build::<i16>(feed),
            SampleFormat::I32 => self.build::<i32>(feed),
            SampleFormat::U16 => self.build::<u16>(feed),
            SampleFormat::U8 => self.build::<u8>(feed),
            SampleFormat::I8 => self.build::<i8>(feed),
            other => Err(format!("unsupported sample format {other}")),
        }?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(AudioOut { _stream: stream })
    }

    fn build<T>(&self, feed: AudioFeed) -> Result<cpal::Stream, String>
    where
        T: SizedSample + FromSample<f32>,
    {
        let channels = usize::from(self.config.channels);
        // コールバックの中で毎回確保しないよう、f32 の作業領域を使い回す。
        let mut scratch: Vec<f32> = Vec::new();
        self.device
            .build_output_stream(
                self.config,
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    if scratch.len() != data.len() {
                        scratch.resize(data.len(), 0.0);
                    }
                    feed.fill(&mut scratch, channels);
                    for (out, value) in data.iter_mut().zip(&scratch) {
                        *out = T::from_sample(value.clamp(-1.0, 1.0));
                    }
                },
                // 途切れ（アンダーラン）やデバイスの切り替えは、再生を止めずに続ける。
                |_error| {},
                None,
            )
            .map_err(|error| error.to_string())
    }
}
