//! All things audio.
//!
//! Audio is handled mostly as signals from [dasp_signal]. Input/output is handled by [cpal].

pub mod input;
pub mod output;
pub mod transformers;

use crate::error::AudioError;
use crate::network::VoiceStreamType;
use crate::state::StatePhase;

use futures_util::stream::Stream;
use futures_util::StreamExt;
use mumble_protocol::voice::{VoicePacket, VoicePacketPayload};
use mumble_protocol::Serverbound;
use std::collections::{hash_map::Entry, HashMap};
use std::fmt::Debug;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

use self::input::{AudioInputDevice, DefaultAudioInputDevice};
use self::output::{AudioOutputDevice, ClientStream, DefaultAudioOutputDevice};

/// The sample rate used internally.
const SAMPLE_RATE: u32 = 48000;

/// Input audio state. Input audio is picket up from an [AudioInputDevice] (e.g.
/// a microphone) and sent over the network.
pub struct AudioInput {
    device: DefaultAudioInputDevice,

    /// Outgoing voice packets that should be sent over the network.
    channel_receiver:
        Arc<tokio::sync::Mutex<Box<dyn Stream<Item = VoicePacket<Serverbound>> + Unpin>>>,
}

impl AudioInput {
    pub fn new(
        input_volume: f32,
        disable_noise_gate: bool,
        phase_watcher: watch::Receiver<StatePhase>,
    ) -> Result<Self, AudioError> {
        let mut default =
            DefaultAudioInputDevice::new(input_volume, disable_noise_gate, phase_watcher, 4)?;

        let opus_stream = default
            .sample_receiver()
            .unwrap()
            .enumerate()
            .map(|(i, e)| VoicePacket::Audio {
                _dst: std::marker::PhantomData,
                target: 0,      // normal speech
                session_id: (), // unused for server-bound packets
                seq_num: i as u64,
                payload: VoicePacketPayload::Opus(e.into(), false),
                position_info: None,
            });

        default.play()?;

        let res = Self {
            device: default,
            channel_receiver: Arc::new(tokio::sync::Mutex::new(Box::new(opus_stream))),
        };
        Ok(res)
    }

    pub fn receiver(
        &self,
    ) -> Arc<tokio::sync::Mutex<Box<dyn Stream<Item = VoicePacket<Serverbound>> + Unpin>>> {
        Arc::clone(&self.channel_receiver)
    }

    pub fn set_volume(&self, input_volume: f32) {
        self.device.set_volume(input_volume);
    }
}

impl Debug for AudioInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AudioInput")
            .field("device", &self.device)
            .field("channel_receiver", &"receiver")
            .finish()
    }
}

#[derive(Debug)]
/// Audio output state. The audio is received from each client over the network,
/// decoded, merged and finally played to an [AudioOutputDevice] (e.g. speaker,
/// headphones, ...).
pub struct AudioOutput {
    device: DefaultAudioOutputDevice,
    /// The volume and mute-status of a user ID.
    user_volumes: Arc<Mutex<HashMap<u32, (f32, bool)>>>,

    /// The client stream per user ID. A separate stream is kept for UDP and TCP.
    ///
    /// Shared with [DefaultAudioOutputDevice].
    client_streams: Arc<Mutex<ClientStream>>,
}

impl AudioOutput {
    pub fn new(output_volume: f32) -> Result<Self, AudioError> {
        let user_volumes = Arc::new(Mutex::new(HashMap::new()));

        let default = DefaultAudioOutputDevice::new(output_volume, Arc::clone(&user_volumes))?;
        default.play()?;

        let client_streams = default.client_streams();

        let res = Self {
            device: default,
            client_streams,
            user_volumes,
        };
        Ok(res)
    }

    /// Decodes a voice packet.
    pub fn decode_packet_payload(
        &self,
        stream_type: VoiceStreamType,
        session_id: u32,
        payload: VoicePacketPayload,
    ) {
        self.client_streams
            .lock()
            .unwrap()
            .decode_packet((stream_type, session_id), payload);
    }

    /// Sets the volume of the output device.
    pub fn set_volume(&self, output_volume: f32) {
        self.device.set_volume(output_volume);
    }

    /// Sets the incoming volume of a user.
    pub fn set_user_volume(&self, id: u32, volume: f32) {
        match self.user_volumes.lock().unwrap().entry(id) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().0 = volume;
            }
            Entry::Vacant(entry) => {
                entry.insert((volume, false));
            }
        }
    }

    /// Mutes another user.
    pub fn set_mute(&self, id: u32, mute: bool) {
        match self.user_volumes.lock().unwrap().entry(id) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().1 = mute;
            }
            Entry::Vacant(entry) => {
                entry.insert((1.0, mute));
            }
        }
    }
}
