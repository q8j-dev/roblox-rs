use kira::effect::reverb::{ReverbBuilder, ReverbHandle};
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle, StaticSoundSettings};
use kira::sound::{PlaybackState, Region};
use kira::track::MainTrackBuilder;
use kira::{
    AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Mix, Panning, PlaybackRate, Tween,
};
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoiceId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceState {
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Copy, Debug)]
pub struct PlayParams {
    pub volume: f32,
    pub pitch: f32,
    pub pan: f32,
    pub looped: bool,
    pub start: f64,
}

pub struct Clip {
    data: StaticSoundData,
    pub duration: f64,
}

struct Voice {
    handle: StaticSoundHandle,
    looped: bool,
}

pub struct AudioEngine {
    manager: Option<AudioManager<DefaultBackend>>,
    reverb: Option<ReverbHandle>,
    clips: HashMap<String, Option<Arc<Clip>>>,
    voices: HashMap<VoiceId, Voice>,
    next: u64,
}

const REVERBS: [(f64, f64, f32); 25] = [
    (0.0, 0.0, 0.0),
    (0.75, 0.5, 0.25),
    (0.35, 0.9, 0.2),
    (0.35, 0.9, 0.2),
    (0.6, 0.3, 0.3),
    (0.4, 0.8, 0.15),
    (0.7, 0.3, 0.3),
    (0.85, 0.4, 0.35),
    (0.9, 0.4, 0.35),
    (0.9, 0.2, 0.4),
    (0.92, 0.4, 0.4),
    (0.93, 0.3, 0.4),
    (0.5, 0.9, 0.2),
    (0.7, 0.4, 0.3),
    (0.75, 0.3, 0.35),
    (0.65, 0.5, 0.25),
    (0.8, 0.7, 0.2),
    (0.7, 0.6, 0.2),
    (0.85, 0.6, 0.2),
    (0.8, 0.5, 0.2),
    (0.6, 0.7, 0.15),
    (0.75, 0.5, 0.25),
    (0.85, 0.4, 0.35),
    (0.9, 0.9, 0.5),
    (0.0, 0.0, 0.0),
];

pub fn amplitude_to_decibels(a: f32) -> Decibels {
    if a <= 0.001 {
        Decibels::SILENCE
    } else {
        Decibels((20.0 * a.log10()).max(Decibels::SILENCE.0))
    }
}

impl Default for AudioEngine {
    fn default() -> Self {
        AudioEngine::disabled()
    }
}

impl AudioEngine {
    pub fn disabled() -> AudioEngine {
        AudioEngine {
            manager: None,
            reverb: None,
            clips: HashMap::new(),
            voices: HashMap::new(),
            next: 1,
        }
    }

    pub fn open() -> AudioEngine {
        let mut main = MainTrackBuilder::new();
        let reverb = main.add_effect(ReverbBuilder::new().mix(Mix(0.0)));
        let settings = AudioManagerSettings {
            main_track_builder: main,
            ..Default::default()
        };
        match AudioManager::<DefaultBackend>::new(settings) {
            Ok(manager) => AudioEngine {
                manager: Some(manager),
                reverb: Some(reverb),
                ..AudioEngine::disabled()
            },
            Err(e) => {
                tracing::warn!("no audio output: {e}");
                AudioEngine::disabled()
            }
        }
    }

    pub fn enabled(&self) -> bool {
        self.manager.is_some()
    }

    pub fn clip(&mut self, key: &str, load: impl FnOnce() -> Option<Vec<u8>>) -> Option<Arc<Clip>> {
        if let Some(c) = self.clips.get(key) {
            return c.clone();
        }
        let clip =
            load().and_then(
                |bytes| match StaticSoundData::from_cursor(Cursor::new(bytes)) {
                    Ok(data) => {
                        let duration = data.duration().as_secs_f64();
                        Some(Arc::new(Clip { data, duration }))
                    }
                    Err(e) => {
                        tracing::warn!("{key}: cannot decode sound: {e}");
                        None
                    }
                },
            );
        self.clips.insert(key.to_string(), clip.clone());
        clip
    }

    pub fn cached_duration(&self, key: &str) -> Option<f64> {
        self.clips
            .get(key)
            .and_then(|c| c.as_ref().map(|c| c.duration))
    }

    pub fn play(&mut self, clip: &Clip, params: PlayParams) -> Option<VoiceId> {
        let manager = self.manager.as_mut()?;
        let settings = StaticSoundSettings::new()
            .volume(amplitude_to_decibels(params.volume))
            .playback_rate(PlaybackRate(params.pitch.max(0.0) as f64))
            .panning(Panning(params.pan.clamp(-1.0, 1.0)))
            .start_position(params.start.clamp(0.0, clip.duration.max(0.0)))
            .loop_region(params.looped.then(|| Region::from(0.0..)));
        let data = clip.data.with_settings(settings);
        match manager.play(data) {
            Ok(handle) => {
                let id = VoiceId(self.next);
                self.next += 1;
                self.voices.insert(
                    id,
                    Voice {
                        handle,
                        looped: params.looped,
                    },
                );
                Some(id)
            }
            Err(e) => {
                tracing::warn!("cannot play sound: {e}");
                None
            }
        }
    }

    pub fn state(&self, id: VoiceId) -> VoiceState {
        match self.voices.get(&id).map(|v| v.handle.state()) {
            Some(PlaybackState::Playing | PlaybackState::Resuming | PlaybackState::Pausing) => {
                VoiceState::Playing
            }
            Some(PlaybackState::Paused | PlaybackState::WaitingToResume) => VoiceState::Paused,
            _ => VoiceState::Stopped,
        }
    }

    pub fn position(&self, id: VoiceId) -> f64 {
        self.voices
            .get(&id)
            .map(|v| v.handle.position())
            .unwrap_or(0.0)
    }

    pub fn set_volume(&mut self, id: VoiceId, volume: f32) {
        if let Some(v) = self.voices.get_mut(&id) {
            v.handle
                .set_volume(amplitude_to_decibels(volume), Tween::default());
        }
    }

    pub fn set_pitch(&mut self, id: VoiceId, pitch: f32) {
        if let Some(v) = self.voices.get_mut(&id) {
            v.handle
                .set_playback_rate(PlaybackRate(pitch.max(0.0) as f64), Tween::default());
        }
    }

    pub fn set_pan(&mut self, id: VoiceId, pan: f32) {
        if let Some(v) = self.voices.get_mut(&id) {
            v.handle
                .set_panning(Panning(pan.clamp(-1.0, 1.0)), Tween::default());
        }
    }

    pub fn set_looped(&mut self, id: VoiceId, looped: bool) {
        if let Some(v) = self.voices.get_mut(&id)
            && v.looped != looped
        {
            v.looped = looped;
            v.handle
                .set_loop_region(looped.then(|| Region::from(0.0..)));
        }
    }

    pub fn seek(&mut self, id: VoiceId, seconds: f64) {
        if let Some(v) = self.voices.get_mut(&id) {
            v.handle.seek_to(seconds.max(0.0));
        }
    }

    pub fn pause(&mut self, id: VoiceId) {
        if let Some(v) = self.voices.get_mut(&id) {
            v.handle.pause(Tween::default());
        }
    }

    pub fn resume(&mut self, id: VoiceId) {
        if let Some(v) = self.voices.get_mut(&id) {
            v.handle.resume(Tween::default());
        }
    }

    pub fn stop(&mut self, id: VoiceId) {
        if let Some(mut v) = self.voices.remove(&id) {
            v.handle.stop(Tween::default());
        }
    }

    pub fn release(&mut self, id: VoiceId) {
        self.voices.remove(&id);
    }

    pub fn set_master_volume(&mut self, volume: f32) {
        if let Some(m) = self.manager.as_mut() {
            m.main_track().set_volume(
                amplitude_to_decibels(volume.clamp(0.0, 1.0)),
                Tween::default(),
            );
        }
    }

    pub fn set_reverb(&mut self, preset: u32) {
        if let Some(r) = self.reverb.as_mut() {
            let (feedback, damping, mix) = REVERBS[(preset as usize).min(REVERBS.len() - 1)];
            r.set_feedback(feedback, Tween::default());
            r.set_damping(damping, Tween::default());
            r.set_mix(Mix(mix), Tween::default());
        }
    }
}

pub fn inverse_rolloff(distance: f32, min: f32, max: f32, rolloff_scale: f32) -> f32 {
    let min = min.max(0.001);
    let d = distance.clamp(min, max.max(min));
    min / (min + rolloff_scale.max(0.0) * (d - min))
}
