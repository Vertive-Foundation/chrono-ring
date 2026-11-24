//! The looping alarm tone. It runs on its own thread so it keeps playing
//! whether or not the window is visible.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rodio::source::SineWave;
use rodio::{OutputStream, Sink, Source};

/// Pitch of each beep in hertz. 880 Hz is clear without being harsh.
const TONE_HZ: f32 = 880.0;
/// How long each beep sounds.
const BEEP: Duration = Duration::from_millis(450);
/// Silence between beeps.
const GAP: Duration = Duration::from_millis(350);
/// Loudness from 0.0 to 1.0. Kept moderate so it is not startling.
const VOLUME: f32 = 0.3;

/// Starts and stops the alarm tone. Tauri keeps one of these in its managed state.
#[derive(Default)]
pub struct AlarmSound {
    /// The stop flag of the tone that is playing, if any.
    stop_flag: Mutex<Option<Arc<AtomicBool>>>,
}

impl AlarmSound {
    /// Starts the tone unless it is already playing.
    pub fn start(&self) {
        let Ok(mut slot) = self.stop_flag.lock() else {
            return;
        };
        if slot.is_some() {
            return;
        }

        let flag = Arc::new(AtomicBool::new(false));
        *slot = Some(Arc::clone(&flag));
        thread::spawn(move || play_until_stopped(&flag));
    }

    /// Asks the playing tone to stop. Does nothing if no tone is playing.
    pub fn stop(&self) {
        let Ok(mut slot) = self.stop_flag.lock() else {
            return;
        };
        if let Some(flag) = slot.take() {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

/// Repeats the beep until the flag is set. Runs on its own thread.
fn play_until_stopped(stop: &AtomicBool) {
    // The stream must stay alive for as long as we play, so it lives in this scope.
    let (_stream, handle) = match OutputStream::try_default() {
        Ok(output) => output,
        Err(err) => {
            eprintln!("no audio output, alarm will not sound: {err}");
            return;
        }
    };
    let sink = match Sink::try_new(&handle) {
        Ok(sink) => sink,
        Err(err) => {
            eprintln!("could not start the alarm tone: {err}");
            return;
        }
    };
    sink.set_volume(VOLUME);

    while !stop.load(Ordering::SeqCst) {
        sink.append(SineWave::new(TONE_HZ).take_duration(BEEP));
        thread::sleep(BEEP + GAP);
    }
    sink.stop();
}