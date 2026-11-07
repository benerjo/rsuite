use enum_iterator::Sequence;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Sequence)]
pub enum Note {
    /// C
    C,
    /// C sharp
    Cs,
    /// D
    D,
    /// D sharp
    Ds,
    /// E
    E,
    /// F
    F,
    /// F sharp
    Fs,
    /// G
    G,
    /// G sharp
    Gs,
    /// A
    A,
    /// A sharp
    As,
    /// B
    B,
}

#[allow(non_upper_case_globals)]
impl Note {
    /// D flat
    pub const Db: Note = Note::Cs;
    /// E flat
    pub const Eb: Note = Note::Ds;
    /// G flat
    pub const Gb: Note = Note::Fs;
    /// A flat
    pub const Ab: Note = Note::Gs;
    /// B flat
    pub const Bb: Note = Note::As;

    pub fn get_index(&self) -> u8 {
        match self {
            Note::C => 0,
            Note::Cs => 1,
            Note::D => 2,
            Note::Ds => 3,
            Note::E => 4,
            Note::F => 5,
            Note::Fs => 6,
            Note::G => 7,
            Note::Gs => 8,
            Note::A => 9,
            Note::As => 10,
            Note::B => 11,
        }
    }

    ///Retrive a note from its distance in semitones from C
    pub fn from_semitones(semitones: u8) -> Note {
        match semitones % 12 {
            0 => Note::C,
            1 => Note::Cs,
            2 => Note::D,
            3 => Note::Ds,
            4 => Note::E,
            5 => Note::F,
            6 => Note::Fs,
            7 => Note::G,
            8 => Note::Gs,
            9 => Note::A,
            10 => Note::As,
            11 => Note::B,
            _ => panic!("Implossible to create a note"),
        }
    }
}

impl std::fmt::Display for Note {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Note::C => write!(f, "C"),
            Note::Cs => write!(f, "C#"),
            Note::D => write!(f, "D"),
            Note::Ds => write!(f, "D#"),
            Note::E => write!(f, "E"),
            Note::F => write!(f, "F"),
            Note::Fs => write!(f, "F#"),
            Note::G => write!(f, "G"),
            Note::Gs => write!(f, "G#"),
            Note::A => write!(f, "A"),
            Note::As => write!(f, "A#"),
            Note::B => write!(f, "B"),
        }
    }
}

impl std::ops::Add<Interval> for Note {
    type Output = Note;

    fn add(self, rhs: Interval) -> Self::Output {
        let this = self.get_index();
        let p = this + rhs.semitones();
        Self::from_semitones(p)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DurationModifier {
    None,
    Dotted,
    DoubleDot,
}

///The duration a note can have
#[derive(Debug, Clone, Copy)]
pub enum DurationFraction {
    Whole,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
}

#[derive(Clone, Debug)]
pub struct DurationType {
    frac: DurationFraction,
    modif: DurationModifier,
}

impl DurationType {
    pub fn new(f: DurationFraction, m: DurationModifier) -> DurationType {
        DurationType { frac: f, modif: m }
    }

    pub fn fraction(&self) -> DurationFraction {
        self.frac
    }

    pub fn modifier(&self) -> DurationModifier {
        self.modif
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Sequence)]
pub enum Interval {
    Unison,
    MinorSecond,
    MajorSecond,
    MinorThird,
    MajorThird,
    PerfectFourth,
    AugmentedFourth,
    PerfectFifth,
    MinorSixth,
    MajorSixth,
    MinorSeventh,
    MajorSeventh,
    Other(u8),
}

impl Interval {
    #[allow(non_upper_case_globals)]
    pub const DiminishedFifth: Interval = Interval::AugmentedFourth;

    ///Retrieve the interval between two notes
    pub fn get_interval(low: Note, high: Note) -> Interval {
        let l = low.get_index();
        let h = high.get_index() + 12;
        debug_assert!(h > l);
        Self::from_semitones(h - l)
    }

    ///Create an interval from a given number of semitones
    pub fn from_semitones(semitones: u8) -> Interval {
        match semitones {
            0 => Interval::Unison,
            1 => Interval::MinorSecond,
            2 => Interval::MajorSecond,
            3 => Interval::MinorThird,
            4 => Interval::MajorThird,
            5 => Interval::PerfectFourth,
            6 => Interval::AugmentedFourth,
            7 => Interval::PerfectFifth,
            8 => Interval::MinorSixth,
            9 => Interval::MajorSixth,
            10 => Interval::MinorSeventh,
            11 => Interval::MajorSeventh,
            v => Interval::Other(v),
        }
    }

    pub fn semitones(&self) -> u8 {
        match self {
            Interval::Unison => 0,
            Interval::MinorSecond => 1,
            Interval::MajorSecond => 2,
            Interval::MinorThird => 3,
            Interval::MajorThird => 4,
            Interval::PerfectFourth => 5,
            Interval::AugmentedFourth => 6,
            Interval::PerfectFifth => 7,
            Interval::MinorSixth => 8,
            Interval::MajorSixth => 9,
            Interval::MinorSeventh => 10,
            Interval::MajorSeventh => 11,
            Interval::Other(v) => *v,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Pitch {
    pub note: Note,
    pub octave: u8,
}

impl Pitch {
    ///Retrieve the frequency in Hz of the pitch
    pub fn get_frequency(&self) -> f64 {
        Self::get_frequency_from_index(self.note.get_index() as f64 + 12.0 * self.octave as f64)
    }

    /// Get the frequency of a note from its index
    pub fn get_frequency_from_index(note_index: f64) -> f64 {
        let index = note_index + 1.0;
        let mid_a_freq = 440.0;
        let a5_index: f64 = 4.0 * 12.0 + 10.0;
        let b: f64 = (2.0 as f64).powf(1.0 / 12.0);
        let a: f64 = mid_a_freq / ((2.0 as f64).powf(a5_index / 12.0));
        a * b.powf(index)
    }
}

impl std::ops::Add<Interval> for Pitch {
    type Output = Pitch;

    fn add(self, rhs: Interval) -> Pitch {
        let this = self.note.get_index() + 12 * self.octave;
        let p = this + rhs.semitones();
        Pitch {
            note: Note::from_semitones(p % 12),
            octave: p / 12,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Nature {
    Major,
    Minor,
    Neutral,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Sequence)]
#[allow(non_camel_case_types)]
pub enum Chord {
    M,
    M7,
    MMaj7,
    Sus4,
    Sus2,
    N5,
    m,
    m7,
    DiabolusInMusica,
}

impl std::fmt::Display for Chord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Chord::M => write!(f, ""),
            Chord::M7 => write!(f, "7"),
            Chord::MMaj7 => write!(f, "Maj7"),
            Chord::Sus4 => write!(f, "Sus4"),
            Chord::Sus2 => write!(f, "Sus2"),
            Chord::N5 => write!(f, "5"),
            Chord::m => write!(f, "m"),
            Chord::m7 => write!(f, "m7"),
            Chord::DiabolusInMusica => write!(f, "4+"),
        }
    }
}

impl Chord {
    pub fn intervals(&self) -> &'static [Interval] {
        match self {
            Chord::M => &[
                Interval::Unison,
                Interval::MajorThird,
                Interval::PerfectFifth,
            ],
            Chord::Sus4 => &[
                Interval::Unison,
                Interval::PerfectFourth,
                Interval::PerfectFifth,
            ],
            Chord::Sus2 => &[
                Interval::Unison,
                Interval::MajorSecond,
                Interval::PerfectFifth,
            ],
            Chord::M7 => &[
                Interval::Unison,
                Interval::MajorThird,
                Interval::PerfectFifth,
                Interval::MinorSeventh,
            ],
            Chord::MMaj7 => &[
                Interval::Unison,
                Interval::MajorThird,
                Interval::PerfectFifth,
                Interval::MajorSeventh,
            ],
            Chord::N5 => &[
                Interval::Unison,
                Interval::PerfectFifth,
                Interval::Other(12),
            ],
            Chord::m => &[
                Interval::Unison,
                Interval::MinorThird,
                Interval::PerfectFifth,
            ],
            Chord::m7 => &[
                Interval::Unison,
                Interval::MinorThird,
                Interval::PerfectFifth,
                Interval::MinorSeventh,
            ],
            Chord::DiabolusInMusica => &[
                Interval::Unison,
                Interval::AugmentedFourth,
                Interval::Other(12),
            ],
        }
    }

    pub fn nature(&self) -> Nature {
        match self {
            Chord::M => Nature::Major,
            Chord::M7 => Nature::Major,
            Chord::MMaj7 => Nature::Major,
            Chord::Sus4 => Nature::Neutral,
            Chord::Sus2 => Nature::Neutral,
            Chord::N5 => Nature::Major,
            Chord::m => Nature::Minor,
            Chord::m7 => Nature::Minor,
            Chord::DiabolusInMusica => Nature::Minor,
        }
    }

    pub fn generate_pitches(&self, tonic: &Pitch) -> Vec<Pitch> {
        let intervals = self.intervals();
        let mut pitches = Vec::with_capacity(intervals.len());
        for intv in self.intervals() {
            pitches.push(*tonic + *intv)
        }
        pitches
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Mode {
    Ionian,
    Dorian,
    Phrygian,
    Lydian,
    Mixolydian,
    Aeolian,
    Locrian,
}

impl Mode {
    pub fn nature(&self) -> Nature {
        match &self {
            Mode::Ionian => Nature::Major,
            Mode::Dorian => Nature::Minor,
            Mode::Phrygian => Nature::Minor,
            Mode::Lydian => Nature::Major,
            Mode::Mixolydian => Nature::Major,
            Mode::Aeolian => Nature::Minor,
            Mode::Locrian => Nature::Minor,
        }
    }

    pub fn intervals(&self) -> &'static [Interval] {
        match self {
            Mode::Ionian => &[
                Interval::Unison,
                Interval::MajorSecond,
                Interval::MajorThird,
                Interval::PerfectFourth,
                Interval::PerfectFifth,
                Interval::MajorSixth,
                Interval::MajorSeventh,
            ],
            Mode::Dorian => &[
                Interval::Unison,
                Interval::MajorSecond,
                Interval::MinorThird,
                Interval::PerfectFourth,
                Interval::PerfectFifth,
                Interval::MajorSixth,
                Interval::MinorSeventh,
            ],
            Mode::Phrygian => &[
                Interval::Unison,
                Interval::MinorSecond,
                Interval::MinorThird,
                Interval::PerfectFourth,
                Interval::PerfectFifth,
                Interval::MinorSixth,
                Interval::MinorSeventh,
            ],
            Mode::Lydian => &[
                Interval::Unison,
                Interval::MajorSecond,
                Interval::MajorThird,
                Interval::PerfectFourth,
                Interval::AugmentedFourth,
                Interval::MajorSixth,
                Interval::MajorSeventh,
            ],
            Mode::Mixolydian => &[
                Interval::Unison,
                Interval::MajorSecond,
                Interval::MajorThird,
                Interval::PerfectFourth,
                Interval::PerfectFifth,
                Interval::MajorSixth,
                Interval::MinorSeventh,
            ],
            Mode::Aeolian => &[
                Interval::Unison,
                Interval::MajorSecond,
                Interval::MinorThird,
                Interval::PerfectFourth,
                Interval::PerfectFifth,
                Interval::MinorSixth,
                Interval::MinorSeventh,
            ],
            Mode::Locrian => &[
                Interval::Unison,
                Interval::MinorSecond,
                Interval::MinorThird,
                Interval::PerfectFourth,
                Interval::DiminishedFifth,
                Interval::MinorSixth,
                Interval::MinorSeventh,
            ],
        }
    }

    pub fn characteristic_interval(&self) -> Interval {
        match self {
            Mode::Ionian => Interval::Unison,
            Mode::Dorian => Interval::MajorSixth,
            Mode::Phrygian => Interval::MinorSecond,
            Mode::Lydian => Interval::AugmentedFourth,
            Mode::Mixolydian => Interval::MinorSeventh,
            Mode::Aeolian => Interval::MinorSixth,
            Mode::Locrian => Interval::DiminishedFifth,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Scale {
    pub tonic: Note,
    pub mode: Mode,
}

impl Scale {
    pub fn notes(&self) -> Vec<Note> {
        let mut notes = Vec::with_capacity(7);
        for i in self.mode.intervals() {
            notes.push(self.tonic + *i)
        }
        notes
    }

    pub fn chord_in_scale(&self, chord_tonic: &Note, chord: &Chord) -> bool {
        let notes = self.notes();
        for int in chord.intervals() {
            let chord_note = *chord_tonic + *int;
            if !notes.contains(&chord_note) {
                return false;
            }
        }
        return true;
    }

    pub fn generate_all_compatible_chords(&self) -> Vec<(Note, Chord)> {
        let mut result = Vec::new();
        for note in enum_iterator::all::<Note>() {
            for c in enum_iterator::all::<Chord>() {
                if self.chord_in_scale(&note, &c) {
                    result.push((note, c))
                }
            }
        }
        result
    }

    pub fn in_scale(&self, chord: &[Pitch]) -> bool {
        let notes = self.notes();
        for c in chord {
            if !notes.contains(&c.note) {
                return false;
            }
        }
        return true;
    }
}
