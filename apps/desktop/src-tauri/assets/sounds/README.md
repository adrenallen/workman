# Bundled notification sound

`dsitempickup.wav` is the Doom pickup sound supplied for Workman as `dsitemup.wav`.
It is the only bundled notification sound, displayed as **Doom**. **System default**
remains the initial selection.

The source was unsigned 8-bit mono PCM at 11,025 Hz (2,263 frames). The bundled copy
uses signed 16-bit PCM with each sample mapped exactly as `(sample - 128) * 256`;
sample rate, timing, and amplitude are preserved. It uses the same validated native
notification path as imported WAV files.

The audio is embedded in the Rust binary with `include_bytes!`, so release/dev
builds need no download or external source file. Selecting Doom installs an owned
copy into the OS notification sound location without changing the signed bundle.
