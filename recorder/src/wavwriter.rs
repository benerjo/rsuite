struct PairIter<T> {
    a: Option<T>,
    b: Option<T>,
}

impl<T> PairIter<T> {
    fn new(pair: (T, T)) -> Self {
        Self {
            a: Some(pair.0),
            b: Some(pair.1),
        }
    }
}

impl<T> Iterator for PairIter<T>
where
    T: Copy,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        match self.a {
            Some(a) => {
                self.a = None;
                Some(a)
            }
            None => match self.b {
                Some(b) => {
                    self.b = None;
                    Some(b)
                }
                None => None,
            },
        }
    }
}

const WAV_FORMAT_PCM: u16 = 0x01;

fn get_wav_header(
    audio_format: u16,
    channel_count: u16,
    sampling_rate: u32,
    bits_per_sample: u16,
) -> [u8; 16] {
    let bpsec = u32::from((bits_per_sample >> 3) * channel_count) * sampling_rate;
    let bpsamp = (bits_per_sample >> 3) * channel_count;

    let mut v: [u8; 16] = [0; 16];
    let b = audio_format.to_le_bytes();
    v[0] = b[0];
    v[1] = b[1];
    let b = channel_count.to_le_bytes();
    v[2] = b[0];
    v[3] = b[1];
    let b = sampling_rate.to_le_bytes();
    v[4] = b[0];
    v[5] = b[1];
    v[6] = b[2];
    v[7] = b[3];
    let b = bpsec.to_le_bytes();
    v[8] = b[0];
    v[9] = b[1];
    v[10] = b[2];
    v[11] = b[3];
    let b = bpsamp.to_le_bytes();
    v[12] = b[0];
    v[13] = b[1];
    let b = bits_per_sample.to_le_bytes();
    v[14] = b[0];
    v[15] = b[1];
    v
}

pub fn save_wav(to_save: Vec<i16>, rate: u32, prefix: Option<&str>) -> Result<(), std::io::Error> {
    let now = chrono::offset::Local::now();
    let mut out_file = std::fs::File::create(std::path::Path::new(&format!(
        "{}-{}.wav",
        prefix.unwrap_or("output"),
        now.format("%Y%m%d%H%M%S")
    )))?;

    let header = get_wav_header(WAV_FORMAT_PCM, 1, rate, 16);

    const WAVE_ID: riff::ChunkId = riff::ChunkId {
        value: [b'W', b'A', b'V', b'E'],
    };
    const HEADER_ID: riff::ChunkId = riff::ChunkId {
        value: [b'f', b'm', b't', b' '],
    };
    const DATA_ID: riff::ChunkId = riff::ChunkId {
        value: [b'd', b'a', b't', b'a'],
    };

    let h_dat = riff::ChunkContents::Data(HEADER_ID, Vec::from(header));

    let d_vec = to_save
        .iter()
        .flat_map(|s| {
            let v = s.to_le_bytes();
            PairIter::new((v[0], v[1]))
        })
        .collect::<Vec<_>>();
    let d_dat = riff::ChunkContents::Data(DATA_ID, d_vec);

    let r = riff::ChunkContents::Children(riff::RIFF_ID.clone(), WAVE_ID, vec![h_dat, d_dat]);

    r.write(&mut out_file)?;

    Ok(())
}
