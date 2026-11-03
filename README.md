# RSuite #
This repository contains the crate RSuite.
The purpose of this crate is to collect small rust programs that enable music creation.

In order to perform the music, the [JACK Audio Connection Kit](https://jackaudio.org/) is used.

* [Programs](#programs)
    * [Synths](#synths)
        * [Kick](#kick)
        * [RSynth](#rsynth)
        * [Snare](#snare)
    * [Effects](#effects)
        * [Chorus](#chorus)
        * [Distortion](#distortion)
        * [Smooth](#smooth)
    * [Utils](#utils)
        * [Activator](#activator)
        * [Loop](#loop)
        * [Metronome](#metronome)
        * [Recorder](#recorder)
        * [Transposer](#transposer)
* [Project](#project)


## Programs

The main idea is to provide small composable programs.
For every program, the different parameters can be set through the user interface or dynamically throuh midi controls.
To define the midi control to use for the different parameters, use the settings menu and locate the parameter for which you which to define the midi control.

The different programs also have an area reseved for error messages.
If error messages would appear, feel free to log an issue.
Messages can be cleared with a dedicated button.

Once a program is running, it is possible to start any other one using the application menu.
The different programs are sorted by their categories.

Note that it is not the purpose of the maintainers to have a polished UI, nor to have the best in class for every program.
The main purpose of the existance of those program is to learn and have fun.

### Synths

A collection of program that are meant to generate music/sounds.

#### Kick
A kick generator.
Takes midi as input and produces audio.

The different elements taht can be configured:
* The wave type used (sin/square/sawtooth/triangle).
Note that it doesn't seems to change a lot
* Duration in frames (typically, there are 44100 frames per seconds)
* Volume
* Start Frequency: the frequency from wich the kicker will start
* End Frequency: the frenquency to wich the kicker will go
* Fade in: the duration (in frames) of the fade in
* Fade out: the duration (in frames) of the fade out

#### RSynth
A customizable synthetizer.
Takes midi as input and produces audio.

The different elements that can be configured:
* The wave type used (sin/square/sawtooth/triangle)
* The relative volume of a few overtones/undertones 
* The duration and shape of the fade-in
* The duration and shape of the fade-out
* The intensity and speed of the frequence modulation

#### Snare

WIP

### Effects

A collection of effects on audio streams

#### Chorus

Mix the audio signal with delayed copies of itself (the voices) to make it sound thicker.
The delay of each voice slowly moves up and down, and the voices are evenly spread over this movement.

The different elements that can be configured:
* Delay: the shortest delay of a voice, in milliseconds
* Depth: the amount of time added to the delay by the modulation, in milliseconds
* Speed: the speed of the modulation, in hertz
* Voices: the number of delayed copies of the signal. Without voice, only the input signal is heard
* Mix: the proportion of delayed signal in the output, between 0 and 1

#### Distortion

Amplify the audio signal and clip it smoothly, which adds harmonics to the sound.
A low pass filter is then applied to tame the highest harmonics.

The different elements that can be configured:
* Drive: the amount of amplification before the signal is clipped
* Tone: the cutoff frequency of the low pass filter, in hertz. At 0, no sound goes through
* Volume: the volume of the output, between 0 and 1

#### Smooth

Apply method to average the audio signal.
At each frame, the audio sample that will be added to the output buffer is using the following formula:

b[i] = alpha * value + (1 - alpha) * b[i - 1]

where alpha is a parameter between 0 and 1, value is the received sample, b[i] is the sample that will be put to the output buffer and b[i-1] is the previous sample in the output buffer.

The different elements taht can be configured:
* Alpha

### Utils

A collection of utilities.

#### Activator

This utility aims to either let midi command through or block them.

The different elements that can be configured:
* If the midi messages are going through or are blocked

#### Loop

Record the midi commands received during a given number of bars and replay them until the loop is de-activated.

It has two midi inputs: the first one (clock_in) should be connected to the midi output of the metronome and the second one (midi_in) receives the midi commands to record.
The midi output only contains the loop. To hear what is played during the recording, also connect the source of the midi commands to the instrument.

Once activated, the recording starts at the beginning of the next bar. The stored loop is forgotten when de-activating.
Stopping the metronome silences the loop, starting it again replays the loop from its beginning.

The different elements that can be configured:
* If the loop is active
* The number of bars of the loop
* The number of quarters per bar. It should be the same as in the metronome

#### Metronome

Provide a audio cue for the rythm

It also sends the midi clock on its midi output: the clock runs continuously, and the start/stop messages follow the activation of the metronome.

The different elements that can be configured:
* The number of quarters per bar
* The number of quarters per minute
* If the metronome is active

#### Recorder

This utility aims to record some audio output on a single channel

#### Transposer

This utility transposes every midi note-on by a given number of half-step


# Project

This project is developed under the GPLv3.
For support (new features/bugs), please fill in an issue.

Note by by default, PR will be rejected, no matter the content. Please, open an issue first. Otherwise, feel free to fork the project.