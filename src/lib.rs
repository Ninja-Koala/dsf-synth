use lv2::prelude::*;
use std::collections::HashMap;
use wmidi::*;

#[derive(PortCollection)]
pub struct Ports {
    attack: InputPort<Control>,
    decay: InputPort<Control>,
    sustain: InputPort<Control>,
    release: InputPort<Control>,
    gain: InputPort<Control>,
    brightness: InputPort<Control>,
    base_note: InputPort<Control>,
    num_octaves: InputPort<Control>,
    input_channel: InputPort<Control>,
    midi_input: InputPort<AtomPort>,
    left_audio_output: OutputPort<Audio>,
    right_audio_output: OutputPort<Audio>,
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

#[derive(Debug, Clone)]
pub struct Tone {
    phase_increment: f32,
    time_pressed: u32,
    time_released: Option<u32>,
    velocity: f32,
    phase: f32,
    n: f32,
}

#[derive(Debug, Clone)]
pub struct Adsr {
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
}

#[uri("https://github.com/Ninja-Koala/dsf-synth")]
pub struct Dsfsynth {
    adsr: Adsr,
    gain: f32,
    brightness: f32,
    base_note: u8,
    num_octaves: u8,
    input_channel: Channel,
    urids: URIDs,
    samplerate: f32,
    active_tones: HashMap<u8, Tone>,
    current_frame: u32,
}

fn _dsf_inf(w: f32, u: f32, v: f32) -> f32 {
    (u.sin() - w * (u - v).sin()) / (1f32 + w * w - 2f32 * w * v.cos())
}

fn dsf_n(w: f32, u: f32, v: f32, n: f32) -> f32 {
    (u.sin() - w * (u - v).sin() - w.powf(n+1f32)*((u+(n+1f32)*v).sin()-w*(u+n*v).sin())) / (1f32 + w * w - 2f32 * w * v.cos())
}

fn midi_val_to_time(val: f32) -> f32 {
    (val / 8f32 - 10f32).exp()
}

fn midi_val_to_ratio(val: f32) -> f32 {
    val / 127f32
}

fn midi_vals_to_adsr(attack: f32, decay: f32, sustain: f32, release: f32) -> Adsr {
    Adsr {
        attack: midi_val_to_time(attack),
        decay: midi_val_to_time(decay),
        sustain: midi_val_to_ratio(sustain),
        release: midi_val_to_time(release),
    }
}

fn decibel(val: f32) -> f32 {
    10f32.powf(val * 0.05)
}

fn ads(adsr: &Adsr, time: f32) -> f32 {
    if time < adsr.attack {
        return time / adsr.attack;
    } else {
        let decay_time = adsr.decay * (1f32 - adsr.sustain);
        if time < adsr.attack + decay_time {
            return 1f32 - (time - adsr.attack) / adsr.decay;
        } else {
            return adsr.sustain;
        }
    }
}

fn envelope(tone: &Tone, frame_index: u32, adsr: &Adsr, samplerate: f32) -> Option<f32> {
    if let Some(released) = tone.time_released {
        let time = ((frame_index - released) as f32) / samplerate;
        let time_at_release = ((released - tone.time_pressed) as f32) / samplerate;
        let val_at_release = ads(adsr, time_at_release);
        let release_time = adsr.release * val_at_release;
        if time < release_time {
            return Some(val_at_release - time / adsr.release);
        } else {
            return None;
        }
    } else {
        let time = ((frame_index - tone.time_pressed) as f32) / samplerate;
        return Some(ads(adsr, time));
    }
}

fn shepard_tone(brightness: f32, phase: f32, base_note: u8, num_octaves: f32, note: u8, n: f32) -> f32 {
    let t = (note as f32 - base_note as f32).rem_euclid(12f32 * num_octaves) / (12f32 * num_octaves);

    let pitch_diff = num_octaves.exp2() - 1f32;
    let mut value = brightness.powf(t * pitch_diff) * dsf_n(brightness.powf(1f32 + pitch_diff * t), phase, phase, n);
    for i in 1..(num_octaves.exp2().floor() as i32) {
        value += (t.exp2() - 1f32) * dsf_n(brightness.powf(num_octaves.exp2()), phase * ((i as f32) / num_octaves.exp2()), phase, n)
    }

    return value;
}

impl Dsfsynth {
    fn midi_note_to_phase_increment(&self, note: wmidi::Note, num_octaves: u8) -> f32 {
        let index = self.base_note as i32 + ((u8::from(note) as i32) - (self.base_note as i32)).rem_euclid(12i32 * num_octaves as i32);
        let pitch = ((index as f32 - 69f32) / 12f32).exp2() * 440f32;
        std::f32::consts::TAU * pitch / self.samplerate
    }
}

impl Plugin for Dsfsynth {
    type Ports = Ports;

    type InitFeatures = Features<'static>;
    type AudioFeatures = ();

    fn new(plugin_info: &PluginInfo, features: &mut Features<'static>) -> Option<Self> {
        Some(Self {
            adsr: Adsr {
                attack: -6f32.exp(),
                decay: -6f32.exp(),
                sustain: 64f32 / 127f32,
                release: -6f32.exp(),
            },
            gain: -20f32,
            brightness: 64f32 / 127f32,
            base_note: 69u8,
            num_octaves: 1u8,
            input_channel: Channel::Ch1,
            urids: features.map.populate_collection()?,
            samplerate: plugin_info.sample_rate() as f32,
            active_tones: HashMap::new(),
            current_frame: 0u32,
        })
    }

    fn run(&mut self, ports: &mut Ports, _: &mut (), sample_count: u32) {
        self.adsr = midi_vals_to_adsr(
            *(ports.attack),
            *(ports.decay),
            *(ports.sustain),
            *(ports.release),
        );
        self.gain = *(ports.gain);
        self.brightness = midi_val_to_ratio(*(ports.brightness));
        self.base_note = *(ports.base_note) as u8;
        self.num_octaves = *(ports.num_octaves) as u8;
        self.input_channel =
            wmidi::Channel::from_index(*(ports.input_channel) as u8 - 1u8).unwrap();

        let input_sequence = ports
            .midi_input
            .read(self.urids.atom.sequence, self.urids.unit.beat)
            .unwrap();

        for (_, atom) in input_sequence {
            let message = if let Some(message) = atom.read(self.urids.midi.wmidi, ()) {
                message
            } else {
                continue;
            };

            match message {
                MidiMessage::NoteOn(channel, note, velocity) => {
                    if channel == self.input_channel {
                        self.active_tones.insert(
                            u8::from(note),
                            Tone {
                                phase_increment: self.midi_note_to_phase_increment(note, self.num_octaves),
                                time_pressed: self.current_frame,
                                time_released: None,
                                velocity: midi_val_to_ratio(u8::from(velocity) as f32),
                                phase: 0f32,
                                n: (std::f32::consts::PI / self.midi_note_to_phase_increment(note, self.num_octaves)).floor(),
                            },
                        );
                        //println!("increment: {}", self.midi_note_to_phase_increment(note));
                        //println!("velocity: {}", midi_val_to_ratio(u8::from(velocity) as f32));
                        //println!("samplerate: {}", self.samplerate);
                        //println!("gain: {}", decibel(self.gain));
                        //println!("brightness: {}", self.brightness);
                        //println!("note: {}", u8::from(note));
                        //println!("base_note: {}", self.base_note);
                    }
                }
                MidiMessage::NoteOff(channel, note, _velocity) => {
                    if channel == self.input_channel {
                        if let Some(tone) = self.active_tones.get_mut(&(u8::from(note))) {
                            tone.time_released = Some(self.current_frame);
                        }
                    }
                }
                _ => (),
            }
        }

        let mut frame_index = self.current_frame;
        for (left_out_frame, right_out_frame) in Iterator::zip(
            ports.left_audio_output.iter_mut(),
            ports.right_audio_output.iter_mut(),
        ) {
            let mut value = (0f32, 0f32);
            let mut finished_tones = vec![];
            for (note, tone) in self.active_tones.iter_mut() {
                if let Some(envelope) = envelope(tone, frame_index, &self.adsr, self.samplerate) {
                    let sample = shepard_tone(self.brightness, tone.phase, self.base_note, self.num_octaves as f32, *note, tone.n);
                    value.0 += sample * envelope * decibel(self.gain) * tone.velocity;
                    value.1 += sample * envelope * decibel(self.gain) * tone.velocity;
                    tone.phase += tone.phase_increment; // Don't modulo! I need subtones
                } else {
                    finished_tones.push(note.clone());
                }
            }
            for note in finished_tones {
                self.active_tones.remove(&note);
            }
            *left_out_frame = value.0;
            *right_out_frame = value.1;
            frame_index += 1;
        }
        self.current_frame += sample_count;
    }

    fn activate(&mut self, _features: &mut Features<'static>) {
        self.active_tones = HashMap::new();
        self.current_frame = 0u32;
    }

    fn deactivate(&mut self, _features: &mut Features<'static>) {
        self.active_tones = HashMap::new();
        self.current_frame = 0u32;
    }
}

lv2_descriptors!(Dsfsynth);
