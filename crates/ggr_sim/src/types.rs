//! The world's state. Everything here is plain data: serialisable, hashable, and free of
//! engine types. Ids are indices into append-only vectors that never compact, so an id is
//! stable for the life of a save.

use std::collections::BTreeMap;

use ggr_content::{Attr, LegKind};
use ggr_core::RngStreams;
use serde::{Deserialize, Serialize};

use crate::sched::Scheduler;

pub type CharId = u32;
pub type InstId = u32;
pub type QuestId = u32;
pub type Cell = (i32, i32);

pub const NO_INJURY: i64 = -1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub minute: i64,
    pub rng: RngStreams,
    pub sched: Scheduler,
    pub instances: Vec<Instance>,
    pub east_wing_open: bool,
    pub chars: Vec<Character>,
    pub quests: Vec<Quest>,
    pub guild: Guild,
    pub candidates: Vec<Candidate>,
    pub promotions: Vec<PromotionOffer>,
    pub objectives: Objectives,
    pub stats: Stats,
    pub audit: SlotAudit,
    pub next_candidate_id: u32,
    /// Sections this build does not understand (a mod's, a newer build's, the view's camera).
    /// Carried through load and save untouched.
    #[serde(skip)]
    pub foreign_sections: BTreeMap<String, (u32, serde_json::Value)>,
}

// ---------------------------------------------------------------------------------------------
// Space

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstanceStatus {
    UnderConstruction { done_at: i64 },
    Ready,
    Demolished,
}

/// A placed prefab: a footprint of blocked cells and the slots people use it from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub id: InstId,
    pub prefab: u16,
    pub origin: Cell,
    pub status: InstanceStatus,
    pub slots: Vec<Slot>,
    /// What the guild paid, for the demolition refund.
    pub paid: i64,
}

impl Instance {
    pub fn is_ready(&self) -> bool {
        self.status == InstanceStatus::Ready
    }
    pub fn staff_slot(&self) -> Option<&Slot> {
        self.slots.iter().find(|s| s.staff)
    }
    /// True when a staff member is physically at the staff slot.
    pub fn is_staffed(&self) -> bool {
        self.staff_slot().is_some_and(|s| s.occupied)
    }
    pub fn free_customer_slot(&self) -> Option<usize> {
        self.slots
            .iter()
            .position(|s| !s.staff && s.holder.is_none())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Slot {
    pub cell: Cell,
    pub staff: bool,
    /// Reserved by (and, once `occupied`, held by) this character.
    pub holder: Option<CharId>,
    pub occupied: bool,
}

/// Reserve -> arrive -> leave, counted. `reserves == releases + held` always.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlotAudit {
    pub reserves: u64,
    pub arrivals: u64,
    pub releases: u64,
    pub cancellations: u64,
}

// ---------------------------------------------------------------------------------------------
// Characters

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CharState {
    Idle,
    Travel,
    Interact,
    /// Off the map: on a quest, off shift, or gone for good.
    OffMap(OffMapReason),
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OffMapReason {
    Quest,
    OffShift,
    /// Quit or dismissed: never returns.
    Left,
}

/// Why a character is walking where they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalkPurpose {
    /// To a reserved slot.
    ToSlot { inst: InstId, slot: u8 },
    /// Their slot was demolished mid-walk; they finish the walk and decide again.
    Stray,
    /// To the door, to leave for a quest.
    DepartQuest,
    /// To the door, shift over.
    EndShift,
    /// To the door, for good.
    Leave,
}

/// A walk the sim has already timed: the presenter draws it, the sim never waits for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Walk {
    pub from: Cell,
    pub to: Cell,
    pub depart: i64,
    pub arrive: i64,
    pub purpose: WalkPurpose,
}

/// What an interacting character is doing, for presenters and for the queue logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Activity {
    None,
    Using {
        inst: InstId,
        slot: u8,
    },
    /// At an unstaffed counter, waiting for service since `since`.
    Queueing {
        inst: InstId,
        slot: u8,
        since: i64,
    },
    Working {
        inst: InstId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shift {
    pub start_hour: u8,
    pub length: u8,
}

impl Shift {
    /// True if `hour` (0..24) falls inside the shift, wrapping past midnight.
    pub fn covers(&self, hour: i64) -> bool {
        let start = i64::from(self.start_hour);
        let end = (start + i64::from(self.length)) % 24;
        if self.length >= 24 {
            return true;
        }
        if start < end {
            hour >= start && hour < end
        } else {
            hour >= start || hour < end
        }
    }
    /// Hours of the shift falling in the night (22:00-06:00).
    pub fn night_hours(&self) -> i32 {
        (0..i64::from(self.length))
            .map(|i| (i64::from(self.start_hour) + i) % 24)
            .filter(|h| *h >= 22 || *h < 6)
            .count() as i32
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaffInfo {
    pub role: u8,
    pub shift: Shift,
    pub happiness: i32,
    pub low_hours: i32,
    /// Minute of the last missed wage, or -1.
    pub unpaid_since: i64,
    /// A station the player pinned them to; otherwise any free station of their role.
    pub workstation: Option<InstId>,
    pub wages_paid: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Character {
    pub id: CharId,
    pub given: u16,
    pub family: u16,
    pub attrs: [i32; Attr::COUNT],
    pub rank: u8,
    pub state: CharState,
    pub state_entered: i64,
    pub serial: u32,
    pub cell: Cell,
    pub walk: Option<Walk>,
    pub activity: Activity,
    pub hunger: i32,
    pub injured_until: i64,
    pub quest: Option<QuestId>,
    pub hired_minute: i64,
    pub staff: Option<StaffInfo>,
    pub adv: Option<AdventurerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdventurerInfo {
    pub xp: i64,
    pub class: u8,
    pub aspiration: Option<u8>,
    /// Percent skill-xp multiplier per branch.
    pub aptitude: Vec<i32>,
    pub skill_xp: Vec<i32>,
    /// One per `ItemSlot`, by index. A two-handed item sits in the main hand and blocks the off.
    pub equipment: [Option<u16>; 5],
    pub quests_at_rank: u32,
    pub quests_succeeded: u32,
    pub quests_failed: u32,
    pub gold_earned: i64,
    /// Minute of the last quest return, to stop the idle loop sending them straight back out.
    pub rested_until: i64,
}

impl Character {
    pub fn is_adventurer(&self) -> bool {
        self.adv.is_some()
    }
    pub fn is_staff(&self) -> bool {
        self.staff.is_some()
    }
    pub fn alive(&self) -> bool {
        self.state != CharState::Dead
    }
    pub fn on_roster(&self) -> bool {
        self.alive() && self.state != CharState::OffMap(OffMapReason::Left)
    }
    pub fn on_map(&self) -> bool {
        matches!(
            self.state,
            CharState::Idle | CharState::Travel | CharState::Interact
        )
    }
    pub fn is_injured(&self, now: i64) -> bool {
        self.injured_until > now
    }
    pub fn name(&self, c: &ggr_content::Content) -> String {
        format!(
            "{} {}",
            c.given_names[self.given as usize], c.family_names[self.family as usize]
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Quests

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuestSource {
    Curated {
        template: u16,
    },
    Procedural {
        category: u8,
        target: u8,
        location: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuestStatus {
    Posted,
    /// Taken; the adventurer is walking to the door.
    Taken,
    Underway,
    Resolved(Outcome),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Succeeded,
    Failed,
    Died,
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestLeg {
    pub kind: LegKind,
    pub minutes: i64,
    pub attr: Attr,
    pub target: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Consequence {
    None,
    Injury,
    Death,
}

/// One pre-rolled leg: the whole printable reason a quest went the way it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegReport {
    pub roll: i32,
    pub attr_value: i32,
    /// Items, skills and class, after the rank's cap.
    pub gear: i32,
    /// Hunger: outside the cap.
    pub food: i32,
    pub target: i32,
    pub passed: bool,
    pub consequence: Consequence,
    pub resolved: bool,
}

impl LegReport {
    pub fn total(&self) -> i32 {
        self.roll + self.attr_value + self.gear + self.food
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quest {
    pub id: QuestId,
    pub source: QuestSource,
    pub rank: u8,
    pub gold: (i32, i32),
    pub xp: i64,
    pub injury_minutes: i64,
    pub legs: Vec<QuestLeg>,
    pub loot: Vec<(u16, i32)>,
    pub posted: i64,
    pub expires: i64,
    pub status: QuestStatus,
    pub taker: Option<CharId>,
    pub departed: i64,
    pub due_back: i64,
    pub report: Vec<LegReport>,
    pub gold_won: i64,
    pub loot_won: Vec<u16>,
    pub resolved_at: i64,
}

impl Quest {
    pub fn is_posted(&self) -> bool {
        self.status == QuestStatus::Posted
    }
    pub fn is_active(&self) -> bool {
        matches!(self.status, QuestStatus::Taken | QuestStatus::Underway)
    }
    pub fn outcome(&self) -> Option<Outcome> {
        match self.status {
            QuestStatus::Resolved(o) => Some(o),
            _ => None,
        }
    }
    pub fn total_minutes(&self) -> i64 {
        self.legs.iter().map(|l| l.minutes).sum()
    }
}

// ---------------------------------------------------------------------------------------------
// Guild, economy, hiring

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoldReason {
    None,
    QuestReward,
    Hire,
    Wage,
    Meal,
    Construction,
    Refund,
    Promotion,
    Purchase,
    Expansion,
}

impl GoldReason {
    pub fn key(self) -> &'static str {
        match self {
            GoldReason::None => "gold.reason.none",
            GoldReason::QuestReward => "gold.reason.quest",
            GoldReason::Hire => "gold.reason.hire",
            GoldReason::Wage => "gold.reason.wage",
            GoldReason::Meal => "gold.reason.meal",
            GoldReason::Construction => "gold.reason.construction",
            GoldReason::Refund => "gold.reason.refund",
            GoldReason::Promotion => "gold.reason.promotion",
            GoldReason::Purchase => "gold.reason.purchase",
            GoldReason::Expansion => "gold.reason.expansion",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guild {
    pub gold: i64,
    /// Count of each item held by the guild, by item index.
    pub stash: Vec<u32>,
    pub renown: i64,
    pub renown_tier: usize,
    pub last_change: i64,
    pub last_reason: GoldReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CandidateStatus {
    Waiting,
    Hired,
    Declined,
    Left,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: u32,
    pub given: u16,
    pub family: u16,
    pub attrs: [i32; Attr::COUNT],
    pub rank: u8,
    /// Some(role) for a staff applicant.
    pub staff_role: Option<u8>,
    pub aptitude: Vec<i32>,
    pub cost: i64,
    pub arrived: i64,
    pub expires: i64,
    pub status: CandidateStatus,
    /// The difficulty floor's free volunteer.
    pub volunteer: bool,
}

impl Candidate {
    pub fn name(&self, c: &ggr_content::Content) -> String {
        format!(
            "{} {}",
            c.given_names[self.given as usize], c.family_names[self.family as usize]
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionOffer {
    pub character: CharId,
    pub to_rank: u8,
    pub fee: i64,
    pub expires: i64,
}

// ---------------------------------------------------------------------------------------------
// Onboarding and bookkeeping

/// The demo's nudge objectives: a checklist, never a gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Objective {
    HireAdventurer,
    WatchReturn,
    BuildCanteen,
    HireCook,
    SetShift,
    EquipItem,
    SetAspiration,
    Promote,
    OpenEastWing,
    ReachRenowned,
}

impl Objective {
    pub const ALL: [Objective; 10] = [
        Objective::HireAdventurer,
        Objective::WatchReturn,
        Objective::BuildCanteen,
        Objective::HireCook,
        Objective::SetShift,
        Objective::EquipItem,
        Objective::SetAspiration,
        Objective::Promote,
        Objective::OpenEastWing,
        Objective::ReachRenowned,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Objective::HireAdventurer => "objective.hire",
            Objective::WatchReturn => "objective.return",
            Objective::BuildCanteen => "objective.canteen",
            Objective::HireCook => "objective.cook",
            Objective::SetShift => "objective.shift",
            Objective::EquipItem => "objective.equip",
            Objective::SetAspiration => "objective.aspire",
            Objective::Promote => "objective.promote",
            Objective::OpenEastWing => "objective.east_wing",
            Objective::ReachRenowned => "objective.renowned",
        }
    }
    pub fn hint_key(self) -> &'static str {
        match self {
            Objective::HireAdventurer => "objective.hire.hint",
            Objective::WatchReturn => "objective.return.hint",
            Objective::BuildCanteen => "objective.canteen.hint",
            Objective::HireCook => "objective.cook.hint",
            Objective::SetShift => "objective.shift.hint",
            Objective::EquipItem => "objective.equip.hint",
            Objective::SetAspiration => "objective.aspire.hint",
            Objective::Promote => "objective.promote.hint",
            Objective::OpenEastWing => "objective.east_wing.hint",
            Objective::ReachRenowned => "objective.renowned.hint",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Objectives {
    pub done: Vec<Objective>,
}

impl Objectives {
    pub fn is_done(&self, o: Objective) -> bool {
        self.done.contains(&o)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Stats {
    pub posted: u32,
    pub taken: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub died: u32,
    pub expired: u32,
    pub hires: u32,
    pub candidates_arrived: u32,
    pub promotions: u32,
    pub meals: u32,
    pub built: u32,
    pub demolished: u32,
    pub quits: u32,
    pub gold_earned: i64,
    pub gold_spent: i64,
    pub stale_events: u64,
    pub max_gold: i64,
}

// ---------------------------------------------------------------------------------------------
// Events out

/// Everything a presenter might want to react to. Raised after the state it reports has
/// committed.
#[derive(Debug, Clone, PartialEq)]
pub enum SimEvent {
    DayStarted {
        day: i64,
    },
    GoldChanged {
        delta: i64,
        reason: GoldReason,
    },
    QuestPosted {
        quest: QuestId,
    },
    QuestTaken {
        quest: QuestId,
        by: CharId,
    },
    QuestDeparted {
        quest: QuestId,
        by: CharId,
    },
    QuestReturned {
        quest: QuestId,
        by: CharId,
        outcome: Outcome,
    },
    QuestExpired {
        quest: QuestId,
    },
    Injured {
        character: CharId,
    },
    Died {
        character: CharId,
        quest: QuestId,
    },
    CandidateArrived {
        candidate: u32,
    },
    CandidateLeft {
        candidate: u32,
    },
    Hired {
        character: CharId,
    },
    PromotionOffered {
        character: CharId,
    },
    Promoted {
        character: CharId,
        rank: u8,
    },
    ClassAttained {
        character: CharId,
        class: u8,
    },
    SkillUp {
        character: CharId,
        skill: u8,
        level: i32,
    },
    StaffQuit {
        character: CharId,
    },
    ShiftStarted {
        character: CharId,
    },
    ShiftEnded {
        character: CharId,
    },
    MealServed {
        character: CharId,
    },
    ConstructionStarted {
        instance: InstId,
    },
    Built {
        instance: InstId,
    },
    Demolished {
        instance: InstId,
    },
    EastWingOpened,
    RenownTierReached {
        tier: usize,
    },
    ObjectiveDone {
        objective: Objective,
    },
    DemoGoalReached,
}
