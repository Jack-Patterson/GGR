//! The YAML as authored: string ids, no cross references resolved. `deny_unknown_fields`
//! everywhere, so a misspelt field is an error rather than a silently ignored default.

use std::collections::BTreeMap;

use ggr_core::GameError;
use serde::Deserialize;

use crate::Attr;

fn parse<T: for<'de> Deserialize<'de>>(name: &str, text: &str) -> Result<T, GameError> {
    serde_yaml::from_str(text).map_err(|e| GameError::content(format!("{name}: {e}")))
}

pub struct RawContent {
    pub rules: RawRules,
    pub ranks: RawRanks,
    pub people: RawPeople,
    pub progression: RawProgression,
    pub building: RawBuilding,
    pub quests: RawQuests,
}

impl RawContent {
    pub fn parse(
        rules: &str,
        ranks: &str,
        people: &str,
        progression: &str,
        building: &str,
        quests: &str,
    ) -> Result<Self, GameError> {
        Ok(RawContent {
            rules: parse("rules.yaml", rules)?,
            ranks: parse("ranks.yaml", ranks)?,
            people: parse("people.yaml", people)?,
            progression: parse("progression.yaml", progression)?,
            building: parse("building.yaml", building)?,
            quests: parse("quests.yaml", quests)?,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRules {
    pub starting_gold: i64,
    pub start_minute: i64,
    pub board_capacity: usize,
    pub quest_generation_interval_minutes: i64,
    pub quest_generation_interval_per_tier: i64,
    pub quest_expiry_minutes: i64,
    pub procedural_quest_percent: i32,
    pub candidate_arrival_min_minutes: i32,
    pub candidate_arrival_max_minutes: i32,
    pub candidate_window_minutes: i64,
    pub candidate_staff_percent: i32,
    pub candidate_staff_percent_when_needed: i32,
    pub hire_cost_base: i64,
    pub hire_cost_per_rank: i64,
    pub hire_cost_per_point: i64,
    pub staff_hire_cost: i64,
    pub injury_default_minutes: i64,
    pub minutes_per_cell_tenths: i64,
    pub idle_dwell_min: i32,
    pub idle_dwell_max: i32,
    pub idle_retry_minutes: i64,
    pub rest_after_quest_minutes: i64,
    pub hunger_start: i32,
    pub hunger_decay_per_hour: i32,
    pub hunger_hungry_below: i32,
    pub hunger_starving_below: i32,
    pub hunger_after_meal: i32,
    pub hunger_after_quest: i32,
    pub well_fed_at_least: i32,
    pub well_fed_leg_bonus: i32,
    pub starving_leg_penalty: i32,
    pub meal_price: i64,
    pub happiness_start: i32,
    pub happiness_base: i32,
    pub happiness_long_shift_from_hours: i32,
    pub happiness_per_long_hour: i32,
    pub happiness_per_night_hour: i32,
    pub happiness_unpaid: i32,
    pub happiness_per_decor: i32,
    pub happiness_decor_max: i32,
    pub happiness_well_fed_staff: i32,
    pub happiness_drift_per_hour: i32,
    pub quit_below: i32,
    pub quit_after_hours: i32,
    pub work_speed_min_percent: i32,
    pub work_speed_max_percent: i32,
    pub infirmary_session_minutes: i64,
    pub infirmary_recovery_multiplier_staffed: i64,
    pub infirmary_recovery_multiplier_unstaffed: i64,
    pub service_patience_minutes: i64,
    pub service_queue_poll_minutes: i64,
    pub training_session_minutes: i64,
    pub training_xp: i32,
    pub training_preference_percent: i32,
    pub demolish_refund_percent: i64,
    pub east_wing_cost: i64,
    pub east_wing_renown_tier: usize,
    pub renown_per_success: Vec<i64>,
    pub guild_share_percent: i64,
    pub upkeep_per_rank: Vec<i64>,
    pub renown_per_death: i64,
    pub renown_tiers: Vec<i64>,
    pub renown_goal_tier: usize,
    pub candidate_rank_weights: Vec<Vec<i32>>,
    pub volunteer_gold_below: i64,
    pub autosave_every_days: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRanks {
    pub offsets: RawOffsets,
    pub consequences: RawConsequenceTable,
    pub ranks: Vec<RawRank>,
    pub demo_rank_cap: usize,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawOffsets {
    pub objective: i32,
    pub encounter: i32,
    pub travel: i32,
    #[serde(rename = "return")]
    pub ret: i32,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConsequenceTable {
    pub objective: RawConsequence,
    pub other: RawConsequence,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConsequence {
    pub nothing: i32,
    pub injury: i32,
    pub death: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRank {
    pub id: String,
    pub letter: String,
    pub attribute_floor: i32,
    pub target_base: i32,
    pub hire_baseline: i32,
    pub modifier_cap: i32,
    pub injury_minutes: i64,
    pub promote_quests: u32,
    pub promote_xp: i64,
    pub promote_fee: i64,
    pub gold_min: i32,
    pub gold_max: i32,
    pub xp: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPeople {
    pub attributes: Vec<RawAttribute>,
    pub given_names: Vec<String>,
    pub family_names: Vec<String>,
    pub staff_roles: Vec<RawRole>,
    pub shift_lengths: Vec<u8>,
    pub starting_roster: RawStartingRoster,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawAttribute {
    pub id: Attr,
    pub name_key: String,
    pub short_key: String,
    pub min: i32,
    pub max: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRole {
    pub id: String,
    pub name_key: String,
    pub station: crate::PrefabKind,
    pub wage_per_hour: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawStartingRoster {
    pub adventurers: Vec<usize>,
    pub staff: Vec<RawStartingStaff>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawStartingStaff {
    pub role: String,
    pub start_hour: u8,
    pub length: u8,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawProgression {
    pub branches: Vec<RawBranch>,
    pub skill_levels: Vec<i32>,
    pub skills: Vec<RawSkill>,
    pub skill_xp_per_leg: i32,
    pub aptitude_min: i32,
    pub aptitude_max: i32,
    pub aspiration_bonus_percent: i32,
    pub classes: Vec<RawClass>,
    pub items: Vec<RawItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBranch {
    pub id: String,
    pub name_key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSkill {
    pub id: String,
    pub branch: String,
    pub name_key: String,
    pub helps: Vec<Attr>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawClass {
    pub id: String,
    pub branch: String,
    pub tier: u8,
    pub name_key: String,
    pub desc_key: String,
    #[serde(default)]
    pub requires: BTreeMap<String, i32>,
    #[serde(default)]
    pub min_rank: usize,
    #[serde(default)]
    pub bonus: BTreeMap<Attr, i32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawItem {
    pub id: String,
    pub name_key: String,
    pub slot: crate::ItemSlot,
    #[serde(default)]
    pub two_handed: bool,
    #[serde(default)]
    pub bonus: BTreeMap<Attr, i32>,
    pub price: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawBuilding {
    pub prefabs: Vec<RawPrefab>,
    pub layout: RawLayout,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPrefab {
    pub id: String,
    pub name_key: String,
    pub desc_key: String,
    pub kind: crate::PrefabKind,
    #[serde(default)]
    pub skills: Vec<String>,
    pub footprint: Vec<[i32; 2]>,
    #[serde(default)]
    pub staff_slot: Option<[i32; 2]>,
    pub slots: Vec<[i32; 2]>,
    pub minutes: i64,
    pub cost: i64,
    pub build_minutes: i64,
    pub renown_tier: usize,
    pub height: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLayout {
    pub width: i32,
    pub height: i32,
    pub door: [i32; 2],
    pub blocked: Vec<[i32; 4]>,
    pub east_wing: RawEastWing,
    pub placements: Vec<RawPlacement>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawEastWing {
    pub wall: [i32; 4],
    pub openings: Vec<[i32; 4]>,
    pub region: [i32; 4],
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPlacement {
    pub prefab: String,
    pub at: [i32; 2],
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawQuests {
    pub curated: Vec<RawQuest>,
    pub procedural: RawProcedural,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawQuest {
    pub id: String,
    pub title_key: String,
    pub desc_key: String,
    pub rank: usize,
    pub gold: [i32; 2],
    pub xp: i64,
    #[serde(default)]
    pub injury_minutes: Option<i64>,
    pub legs: Vec<RawLeg>,
    #[serde(default)]
    pub loot: Vec<RawLoot>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLeg {
    pub kind: crate::LegKind,
    pub minutes: i64,
    pub attr: Attr,
    pub target: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLoot {
    pub item: String,
    pub percent: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawProcedural {
    pub locations: Vec<String>,
    pub categories: Vec<RawCategory>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawCategory {
    pub id: String,
    pub verb_key: String,
    pub targets: Vec<String>,
    pub legs: Vec<(crate::LegKind, Attr)>,
}
