use lv2::prelude::*;
use wmidi::*;

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
}

impl Plugin for Dsfsynth {
    type Ports = Ports;

    type InitFeatures = Features<'static>;
    type AudioFeatures = ();

    fn new(_plugin_info: &PluginInfo, features: &mut Features<'static>) -> Option<Self> {
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

        for (timestamp, atom) in input_sequence {
            // Every message is forwarded, regardless of it's content.
            output_sequence.forward(timestamp, atom);

            let message = if let Some(message) = atom.read(self.urids.midi.wmidi, ()) {
                message
            } else {
                continue;
            };

            match message {
                MidiMessage::ControlChange(channel, number, value) => {
					if channel == self.input_channel {
                        for out_frame in ports.audio_output.iter_mut() {
                            *out_frame = 0f32;
                        }
                    }
                }
                _ => (),
            }
        }
    }

	// not sure if i want this
    fn activate(&mut self, _features: &mut Features<'static>) {
        self.note_active = false;
    }
}

lv2_descriptors!(Dsfsynth);
