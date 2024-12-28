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
        * [Smooth](#smooth)
    * [Utils](#utils)
        * [Activator](#activator)
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

#### Snare

WIP

### Effects

A collection of effects on audio streams

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

#### Recorder

This utility aims to record some audio output on a single channel

#### Transposer

This utility transposes every midi note-on by a given number of half-step


# Project

This project is developed under the GPLv3.
For support (new features/bugs), please fill in an issue.

Note by by default, PR will be rejected, no matter the content. Please, open an issue first. Otherwise, feel free to fork the project.