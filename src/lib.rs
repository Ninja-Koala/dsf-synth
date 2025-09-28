use lv2::prelude::*;
use wmidi::*;
use std::collections::HashMap;

#[derive(PortCollection)]
pub struct Ports {
    attack: InputPort<Control>,
    decay: InputPort<Control>,
    sustain: InputPort<Control>,
    release: InputPort<Control>,
    brightness: InputPort<Control>,
    gain: InputPort<Control>,
    input_channel: InputPort<Control>,
    midi_input: InputPort<AtomPort>,
    midi_output: OutputPort<AtomPort>,
    audio_output: OutputPort<Audio>,
}

#[derive(FeatureCollection)]
pub struct Features<'a> {
    map: LV2Map<'a>,
}

#[derive(URIDCollection)]
pub struct URIDs {
    atom: AtomURIDCollection,
    midi: MidiURIDCollection,
    unit: UnitURIDCollection,
}

#[uri("https://github.com/Ninja-Koala/dsf-synth")]
pub struct Dsfsynth {
	note_active: bool,
	attack: U7,
    decay: U7,
	sustain: U7,
	release: U7,
	brightness: U7,
	gain: f32,
	input_channel: Channel,
    urids: URIDs,
    samplerate: f32,
}

pub struct Tone {
    phase_increment: f32,
    time_pressed: TimeStamp,
    time_released: Option<TimeStamp>,
    velocity: f32,
    phase: f32,
}

fn midi_note_to_pitch(note: wmidi::Note) -> f32 {
    (((u8::from(note) as f32) - 69f32)/12f32).exp2() * 440f32
}

fn dsf_inf(w: f32, u: f32, v: f32) -> f32 {
    (u.sin() - w*(u-v).sin()) / (1f32 + w*w - 2f32 * w * v.cos())
}

fn midi_val_to_time(val: U7) -> f32 {
    ((u8::from(val) as f32) / 8f32 - 10f32).exp()
}

fn midi_val_to_ratio(val: U7) -> f32 {
    (u8::from(val) as f32) / 127f32
}

fn gain(val: f32) -> f32 {
    10.0_f32.powf(val * 0.05)
}

impl Dsfsynth {
    fn phase_increment_from_pitch(&self, pitch: f32) -> f32 {
        std::f32::consts::TAU * pitch / self.samplerate
    }

    fn ads(&self, tone: &Tone, current_time: TimeStamp) -> f32 {
        let time = ((current_time.as_frames().unwrap() - tone.time_pressed.as_frames().unwrap()) as f32) / self.samplerate;
        let attack_time = midi_val_to_time(self.attack);
        if time < attack_time {
            return time / attack_time;
        } else {
            let sustain = midi_val_to_ratio(self.sustain);
            let decay = midi_val_to_time(self.decay);
            let decay_time = decay * (1f32 - sustain);
            if time < attack_time + decay_time {
                return 1f32 - (time-attack_time)/decay;
            } else {
                return sustain;
            }
        }
    }

    fn envelope(&self, tone: &Tone, current_time: TimeStamp) -> Option<f32> {
        if let Some(released) = tone.time_released {
            let time = ((current_time.as_frames().unwrap() - released.as_frames().unwrap()) as f32) / self.samplerate;
            let val_at_release = self.ads(tone, released);
            let release = midi_val_to_time(self.release);
            let release_time = release * val_at_release;
            if time < release_time {
                return Some(val_at_release - time/release);
            } else {
                return None;
            }
        } else {
            return Some(self.ads(tone, current_time));
        }
    }
}

impl Plugin for Dsfsynth {
    type Ports = Ports;

    type InitFeatures = Features<'static>;
    type AudioFeatures = ();

    fn new(plugin_info: &PluginInfo, features: &mut Features<'static>) -> Option<Self> {
        Some(Self {
            note_active: false,
			attack: 64.try_into().unwrap(),
            decay: 50.try_into().unwrap(),
			sustain: 127.try_into().unwrap(),
			release: 127.try_into().unwrap(),
			brightness: 127.try_into().unwrap(),
			gain: 0f32.try_into().unwrap(),
			input_channel: Channel::Ch1,
            urids: features.map.populate_collection()?,
            samplerate: plugin_info.sample_rate() as f32,
        })
    }

    fn run(&mut self, ports: &mut Ports, _: &mut (), _: u32) {
		self.attack = (*(ports.attack) as u8).try_into().unwrap();
		self.decay = (*(ports.decay) as u8).try_into().unwrap();
		self.sustain = (*(ports.sustain) as u8).try_into().unwrap();
		self.release = (*(ports.release) as u8).try_into().unwrap();
		self.brightness = (*(ports.brightness) as u8).try_into().unwrap();
		self.gain = (*(ports.gain) as f32).try_into().unwrap();
		self.input_channel = wmidi::Channel::from_index(*(ports.input_channel) as u8 - 1u8).unwrap();

        let input_sequence = ports
            .midi_input
            .read(self.urids.atom.sequence, self.urids.unit.beat)
            .unwrap();

        let mut output_sequence = ports
            .midi_output
            .init(
                self.urids.atom.sequence,
                TimeStampURID::Frames(self.urids.unit.frame),
            )
            .unwrap();

        let mut active_tones = HashMap::new();
        for (timestamp, atom) in input_sequence {
            // Every message is forwarded, regardless of it's content.
            output_sequence.forward(timestamp, atom);

            let message = if let Some(message) = atom.read(self.urids.midi.wmidi, ()) {
                message
            } else {
                continue;
            };

            match message {
                MidiMessage::NoteOn(channel, note, velocity) => {
					if channel == self.input_channel {
                        active_tones.insert(u8::from(note), Tone {
                            phase_increment: self.phase_increment_from_pitch(midi_note_to_pitch(note)),
                            time_pressed: timestamp,
                            time_released: None,
                            velocity: midi_val_to_ratio(velocity),
                            phase: 0f32,
                        });
                    }
                }
                MidiMessage::NoteOff(channel, note, _velocity) => {
					if channel == self.input_channel {
                        if let Some(tone) = active_tones.get_mut(&u8::from(note)) {
                            tone.time_released = Some(timestamp);
                        }
                    }
                }
                _ => (),
            }
            for out_frame in ports.audio_output.iter_mut() {
                let mut value = 0f32;
                let mut finished_tones = vec![];
                for (note, tone) in active_tones.iter_mut() {
                    if let Some(envelope) = self.envelope(tone, timestamp) {
                        value += tone.phase.sin() * envelope * gain(self.gain) * tone.velocity;
                        tone.phase = (tone.phase + tone.phase_increment).rem_euclid(std::f32::consts::TAU);
                    } else {
                        finished_tones.push(note.clone());
                    }
                }
                for note in finished_tones {
                    active_tones.remove(&note);
                }
                *out_frame = value;
            }
        }
    }

	// not sure if i want this
    fn activate(&mut self, _features: &mut Features<'static>) {
        self.note_active = false;
    }
}

lv2_descriptors!(Dsfsynth);
