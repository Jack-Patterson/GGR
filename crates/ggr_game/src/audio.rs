//! Audio: short generated tones on the events worth hearing (no audio assets to license). The
//! master volume and the effects switch come from the options menu.

use std::time::Duration;

use bevy::audio::Volume;
use bevy::prelude::*;
use ggr_sim::{Outcome, SimEvent};

use crate::persist::Settings;
use crate::state::*;

pub fn setup() {}

fn tone(commands: &mut Commands, pitches: &mut Assets<Pitch>, freq: f32, ms: u64, vol: f32) {
    commands.spawn((
        AudioPlayer(pitches.add(Pitch::new(freq, Duration::from_millis(ms)))),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(vol)),
    ));
}

pub fn play_events(
    mut commands: Commands,
    mut pitches: ResMut<Assets<Pitch>>,
    mut events: MessageReader<SimEventMsg>,
    settings: Res<Settings>,
) {
    if !settings.sfx || settings.master_volume <= 0.0 {
        events.clear();
        return;
    }
    let v = settings.master_volume * 0.25;
    let mut played = 0;
    for e in events.read() {
        // At high speed many events land in one frame; three tones a frame is plenty.
        if played >= 3 {
            break;
        }
        let (f, ms, vol) = match &e.0 {
            SimEvent::QuestReturned {
                outcome: Outcome::Succeeded,
                ..
            } => (660.0, 120, v),
            SimEvent::QuestReturned { .. } => (330.0, 160, v),
            SimEvent::Died { .. } => (196.0, 600, v * 1.2),
            SimEvent::CandidateArrived { .. } => (523.0, 90, v * 0.7),
            SimEvent::Hired { .. } => (784.0, 120, v),
            SimEvent::Built { .. } => (440.0, 140, v),
            SimEvent::Promoted { .. } | SimEvent::ClassAttained { .. } => (880.0, 200, v),
            SimEvent::RenownTierReached { .. } | SimEvent::DemoGoalReached => (988.0, 300, v),
            SimEvent::StaffQuit { .. } => (220.0, 300, v),
            _ => continue,
        };
        tone(&mut commands, &mut pitches, f, ms, vol);
        played += 1;
    }
}
