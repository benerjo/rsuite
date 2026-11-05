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
        * [Reverb](#reverb)
        * [Smooth](#smooth)
    * [Utils](#utils)
        * [Activator](#activator)
        * [Loop](#loop)
        * [Metronome](#metronome)
        * [Recorder](#recorder)
        * [Transposer](#transposer)
    * [Session management](#session-management)
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

#### Reverb

Simulate the reflections of the sound on the walls of a room.
After the pre-delay, the audio signal goes through several delays whose output is fed back to their input: each one produces echoes that slowly fade away.
The echoes are then spread so that they are no longer heard one by one.

The different elements that can be configured:
* Size: the size of the room, between 0 and 1. The bigger the room, the longer the reverberation
* Damping: how much the high frequencies are absorbed by the walls, between 0 and 1
* Pre-delay: the time before the first echo is heard, in milliseconds
* Mix: the proportion of reverberated signal in the output, between 0 and 1

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

Once activated, the recording starts at the beginning of the next bar, so a loop always starts with the first beat of a bar.
The beginning of the bars is given by the bar markers and the number of quarters per bar by the time signature, both sent by the metronome.
The stored loop is forgotten when de-activating.

Once recorded, the replay of the loop is controlled without recording a new loop:
* Play: starts the replay again with the next bar, from the beginning of the loop. While stopping, it keeps repeating the loop instead
* Stop: finishes the current replay of the loop, without repeating it
* Break: stops the replay immediately
Stopping the metronome silences the loop, starting it again replays the loop from its beginning.

The different elements that can be configured:
* If the loop is active
* If the recorded loop is replayed, stopped or interrupted (play/stop/break)
* The number of bars of the loop
* The number of quarters per bar. It is updated by the time signature received from the metronome

#### Metronome

Provide a audio cue for the rythm

It also sends the midi clock on its midi output: the clock runs continuously, and the start/stop messages follow the activation of the metronome.
While active, it also sends the time signature (when starting and whenever the number of quarters per bar changes) and a bar marker at the start of every bar.

The different elements that can be configured:
* The number of quarters per bar
* The number of quarters per minute
* If the metronome is active

#### Recorder

This utility aims to record some audio output on a single channel.
When it is used in a session, the audio files are written in the directory of the session.

#### Transposer

This utility transposes every midi note-on by a given number of half-step

### Session management

Every program can be managed by the [New Session Manager](https://new-session-manager.jackaudio.org/) (NSM), or by any other session manager implementing its protocol.
When started by a session manager, a program:
* uses the JACK client name chosen by the session manager, so that the connections can be restored
* restores its state when it starts and saves it when the session is saved. The state contains every parameter that can be configured and the midi controls that are mapped to them
* restores the position and size of its window. If the saved position is no longer visible (screen resolution changed, monitor unplugged...), the window is moved back on a screen and shrunk to fit it
* shows its window with the title 'client name - client id'
* quits when the session is closed

The state is written in the file 'state.conf', in the directory of the program inside the session.
The recorded loop of the Loop utility is not part of the state.

Without session manager, nothing is saved and the programs behave as usual.


# Project

This project is developed under the GPLv3.
For support (new features/bugs), please fill in an issue.

Note by by default, PR will be rejected, no matter the content. Please, open an issue first. Otherwise, feel free to fork the project.