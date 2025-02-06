use rsuite::midiinput::MidiInput;

#[derive(Clone, Debug)]
struct RawMidiCopy {
    #[allow(dead_code)]
    time: u32,
    data: [u8; 4],
}

impl<'d> From<&jack::RawMidi<'d>> for RawMidiCopy {
    fn from(value: &jack::RawMidi<'d>) -> Self {
        let mut out = RawMidiCopy {
            time: value.time,
            data: [0, 0, 0, 0],
        };
        for i in 0..(std::cmp::min(value.bytes.len(), 4)) {
            out.data[i] = value.bytes[i];
        }
        out
    }
}

fn main() {
    // open client
    let (client, _status) =
        jack::Client::new("midi_debug", jack::ClientOptions::NO_START_SERVER).unwrap();

    //create a sync channel to send back copies of midi messages we get
    let (sender, receiver) = std::sync::mpsc::sync_channel(64);

    // process logic
    let mut maker = client
        .register_port("midi_out", jack::MidiOut::default())
        .unwrap();
    let shower = client
        .register_port("midi_in", jack::MidiIn::default())
        .unwrap();

    let cback = move |_: &jack::Client, ps: &jack::ProcessScope| -> jack::Control {
        let show_p = shower.iter(ps);
        let mut put_p = maker.writer(ps);
        for e in show_p {
            let c: MidiInput = e.into();
            let _ = sender.try_send((c, RawMidiCopy::from(&e)));
            match put_p.write(&e) {
                Ok(()) => {}
                Err(e) => eprintln!("Error while trying to pass midi: {}", e),
            };
        }
        jack::Control::Continue
    };

    // activate
    let active_client = client
        .activate_async((), jack::contrib::ClosureProcessHandler::new(cback))
        .unwrap();

    //spawn a non-real-time thread that prints out the midi messages we get
    std::thread::spawn(move || {
        while let Ok(m) = receiver.recv() {
            println!("{m:?}");
        }
    });

    // wait
    println!("Press any key to quit");
    let mut user_input = String::new();
    std::io::stdin().read_line(&mut user_input).ok();

    // optional deactivation
    active_client.deactivate().unwrap();
}
