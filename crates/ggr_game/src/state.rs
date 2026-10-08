//! The game's resources and the two systems that pump the sim: advancing time and turning sim
//! events into notices and messages.

use std::collections::VecDeque;
use std::sync::Arc;

use bevy::prelude::*;
use ggr_content::Content;
use ggr_sim::{CharId, InstId, Outcome, QuestId, SimEvent, World};

/// The sealed content, available before any world exists (menus need strings too).
#[derive(Resource, Clone)]
pub struct ContentRes(pub Arc<Content>);

impl ContentRes {
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.0.loc.t(key)
    }
    pub fn f(&self, key: &str, args: &[&str]) -> String {
        self.0.loc.f(key, args)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    MainMenu,
    Playing,
}

/// The running game, if any. On the main menu the world is an attract-mode guild running in
/// the background.
#[derive(Resource, Default)]
pub struct Session {
    pub world: Option<World>,
    pub mode: Mode,
    /// Bumped whenever `world` is replaced, so presenters rebuild from scratch.
    pub generation: u32,
    pub goal_announced: bool,
}

impl Session {
    pub fn replace(&mut self, world: World, mode: Mode) {
        self.world = Some(world);
        self.mode = mode;
        self.generation = self.generation.wrapping_add(1);
        self.goal_announced = false;
    }
}

/// Game speed. The sim only ever advances whole minutes; the fraction is kept so walks
/// interpolate smoothly between them.
#[derive(Resource)]
pub struct Pace {
    pub speed: u32,
    pub paused: bool,
    pub frac: f64,
}

impl Default for Pace {
    fn default() -> Self {
        Self {
            speed: 1,
            paused: false,
            frac: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sheet {
    Character(CharId),
    Candidate(u32),
    Quest(QuestId),
    Instance(InstId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RosterTab {
    #[default]
    Adventurers,
    Staff,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RightTab {
    #[default]
    Board,
    Chronicle,
    Outfitter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuPage {
    Pause,
    Save,
    Load,
    Options,
    HowTo,
    Confirm(ConfirmAction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmAction {
    MainMenu,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BuildState {
    pub prefab: Option<usize>,
    pub demolish: bool,
}

/// Everything the HUD remembers between frames.
#[derive(Resource, Default)]
pub struct UiState {
    pub selected_character: Option<CharId>,
    pub selected_instance: Option<InstId>,
    /// The modal detail sheet, with at most one level of Back.
    pub sheet: Vec<Sheet>,
    pub roster_tab: RosterTab,
    pub rank_filter: Option<u8>,
    pub injured_only: bool,
    pub show_dead: bool,
    pub search: String,
    pub right_tab: RightTab,
    pub underway_tab: bool,
    pub build: Option<BuildState>,
    pub menu: Option<MenuPage>,
    pub menu_return: Option<MenuPage>,
    pub debug: bool,
    pub rebinding: Option<crate::input::Action>,
    pub hover_cell: Option<(i32, i32)>,
    pub ghost_ok: Option<Result<(), ggr_sim::Refusal>>,
    pub objectives_open: bool,
    pub goal_dialog: bool,
    pub status: Option<String>,
}

impl UiState {
    pub fn open_sheet(&mut self, s: Sheet) {
        self.sheet.clear();
        self.sheet.push(s);
    }
    /// Opens a sheet one level deep from the current one (Back returns).
    pub fn link_sheet(&mut self, s: Sheet) {
        if self.sheet.len() >= 2 {
            self.sheet.truncate(1);
        }
        if self.sheet.last() != Some(&s) {
            self.sheet.push(s);
        }
    }
    pub fn modal_open(&self) -> bool {
        !self.sheet.is_empty() || self.menu.is_some() || self.goal_dialog
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Gain,
    Alert,
}

/// The notice line and the chronicle: what the guild has heard, newest first.
#[derive(Resource, Default)]
pub struct Notices {
    pub log: VecDeque<(i64, String, Tone)>,
    pub current: Option<(String, Tone, f64)>,
}

impl Notices {
    pub fn push(&mut self, minute: i64, text: String, tone: Tone, now_secs: f64) {
        self.current = Some((text.clone(), tone, now_secs + 6.0));
        self.log.push_front((minute, text, tone));
        self.log.truncate(200);
    }
}

#[derive(Message, Clone, Debug)]
pub struct SimEventMsg(pub SimEvent);

/// Real seconds per game-minute at 1x.
const SECONDS_PER_MINUTE: f64 = 1.0;

pub fn advance_sim(
    time: Res<Time>,
    mut session: ResMut<Session>,
    mut pace: ResMut<Pace>,
    ui: Res<UiState>,
) {
    let mode = session.mode;
    let Some(world) = session.world.as_mut() else {
        return;
    };
    let (speed, paused) = match mode {
        Mode::MainMenu => (4, false),
        // The pause menu stops time; the detail sheet does not (V2: decisions are made under
        // the clock, which is what makes a hiring window a window).
        Mode::Playing => (
            pace.speed,
            pace.paused || ui.menu.is_some() || ui.goal_dialog,
        ),
    };
    if paused {
        return;
    }
    pace.frac += time.delta_secs_f64() * f64::from(speed) / SECONDS_PER_MINUTE;
    // Never more than a few game-hours in one frame, so a stalled frame cannot spiral.
    let whole = (pace.frac.floor() as i64).min(240);
    if whole > 0 {
        world.advance(whole);
        pace.frac -= whole as f64;
        if pace.frac > 1.0 {
            pace.frac = pace.frac.fract();
        }
    }
}

pub fn dispatch_events(
    mut session: ResMut<Session>,
    content: Res<ContentRes>,
    mut notices: ResMut<Notices>,
    mut out: MessageWriter<SimEventMsg>,
    time: Res<Time>,
    mut ui: ResMut<UiState>,
) {
    let mode = session.mode;
    let Some(world) = session.world.as_mut() else {
        return;
    };
    let events = world.drain_events();
    if mode != Mode::Playing {
        return;
    }
    let now = time.elapsed_secs_f64();
    let minute = world.minute();
    let mut goal = false;
    for e in events {
        if let Some((text, tone)) = describe(world, &content, &e) {
            notices.push(minute, text, tone, now);
        }
        if e == SimEvent::DemoGoalReached {
            goal = true;
        }
        out.write(SimEventMsg(e));
    }
    if goal && !session.goal_announced {
        session.goal_announced = true;
        ui.goal_dialog = true;
    }
}

/// A sim event as a sentence for the notice line, or None if it is not news.
pub fn describe(world: &World, c: &ContentRes, e: &SimEvent) -> Option<(String, Tone)> {
    let name = |id: CharId| world.characters()[id as usize].name(&c.0);
    let quest = |id: QuestId| world.quest_title(&world.quests()[id as usize]);
    Some(match e {
        SimEvent::DayStarted { day } => (c.f("notice.day", &[&day.to_string()]), Tone::Plain),
        SimEvent::QuestPosted { quest: q } => (c.f("notice.posted", &[&quest(*q)]), Tone::Plain),
        SimEvent::QuestTaken { quest: q, by } => {
            (c.f("notice.taken", &[&name(*by), &quest(*q)]), Tone::Plain)
        }
        SimEvent::QuestReturned {
            quest: q,
            by,
            outcome,
        } => match outcome {
            Outcome::Succeeded => {
                let gold = world.quests()[*q as usize].gold_won;
                (
                    c.f(
                        "notice.succeeded",
                        &[&name(*by), &quest(*q), &gold.to_string()],
                    ),
                    Tone::Gain,
                )
            }
            _ => (c.f("notice.failed", &[&name(*by), &quest(*q)]), Tone::Alert),
        },
        SimEvent::Died {
            character,
            quest: q,
        } => (
            c.f("notice.died", &[&name(*character), &quest(*q)]),
            Tone::Alert,
        ),
        SimEvent::Injured { character } => {
            (c.f("notice.injured", &[&name(*character)]), Tone::Alert)
        }
        SimEvent::CandidateArrived { .. } => (c.t("notice.candidate").to_string(), Tone::Plain),
        SimEvent::Hired { character } => (c.f("notice.hired", &[&name(*character)]), Tone::Plain),
        SimEvent::PromotionOffered { character } => (
            c.f("notice.promotion_offered", &[&name(*character)]),
            Tone::Gain,
        ),
        SimEvent::Promoted { character, rank } => (
            c.f(
                "notice.promoted",
                &[&name(*character), &c.0.ranks[*rank as usize].letter],
            ),
            Tone::Gain,
        ),
        SimEvent::ClassAttained { character, class } => (
            c.f(
                "notice.class",
                &[
                    &name(*character),
                    c.t(&c.0.classes[*class as usize].name_key),
                ],
            ),
            Tone::Gain,
        ),
        SimEvent::SkillUp {
            character,
            skill,
            level,
        } => (
            c.f(
                "notice.skill",
                &[
                    &name(*character),
                    c.t(&c.0.skills[*skill as usize].name_key),
                    &level.to_string(),
                ],
            ),
            Tone::Plain,
        ),
        SimEvent::StaffQuit { character } => {
            (c.f("notice.quit", &[&name(*character)]), Tone::Alert)
        }
        SimEvent::Built { instance } => {
            let p = world.instances()[*instance as usize].prefab as usize;
            (
                c.f("notice.built", &[c.t(&c.0.prefabs[p].name_key)]),
                Tone::Plain,
            )
        }
        SimEvent::EastWingOpened => (c.t("notice.east_wing").to_string(), Tone::Gain),
        SimEvent::RenownTierReached { tier } => (
            c.f("notice.renown", &[c.t(&format!("renown.tier.{tier}"))]),
            Tone::Gain,
        ),
        SimEvent::ObjectiveDone { objective } => {
            (c.f("notice.objective", &[c.t(objective.key())]), Tone::Gain)
        }
        _ => return None,
    })
}
