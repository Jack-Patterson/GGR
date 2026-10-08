//! Validation and resolution: raw string ids become indices, and every rule the content must
//! obey is checked once, at load, with an error that names the file, the offender and the
//! valid set.

use std::collections::{BTreeMap, BTreeSet};

use ggr_core::{fnv1a64, GameError};
use serde::{Deserialize, Serialize};

use crate::raw::*;
use crate::{Attr, Loc};

pub type Rules = RawRules;
pub type Offsets = RawOffsets;
pub type ConsequenceWeights = RawConsequence;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegKind {
    Travel,
    Encounter,
    Objective,
    Return,
}

impl LegKind {
    pub fn token(self) -> &'static str {
        match self {
            LegKind::Travel => "travel",
            LegKind::Encounter => "encounter",
            LegKind::Objective => "objective",
            LegKind::Return => "return",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemSlot {
    Head,
    Body,
    Legs,
    MainHand,
    OffHand,
}

impl ItemSlot {
    pub const ALL: [ItemSlot; 5] = [
        ItemSlot::Head,
        ItemSlot::Body,
        ItemSlot::Legs,
        ItemSlot::MainHand,
        ItemSlot::OffHand,
    ];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn key(self) -> &'static str {
        match self {
            ItemSlot::Head => "slot.head",
            ItemSlot::Body => "slot.body",
            ItemSlot::Legs => "slot.legs",
            ItemSlot::MainHand => "slot.main_hand",
            ItemSlot::OffHand => "slot.off_hand",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrefabKind {
    Desk,
    Board,
    Rest,
    Training,
    Canteen,
    Infirmary,
    Decor,
}

#[derive(Debug, Clone)]
pub struct AttributeDef {
    pub attr: Attr,
    pub name_key: String,
    pub short_key: String,
    pub min: i32,
    pub max: i32,
}

#[derive(Debug, Clone)]
pub struct Rank {
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

#[derive(Debug, Clone)]
pub struct Role {
    pub id: String,
    pub name_key: String,
    pub station: PrefabKind,
    pub wage_per_hour: i64,
}

#[derive(Debug, Clone)]
pub struct StartingStaff {
    pub role: usize,
    pub start_hour: u8,
    pub length: u8,
}

#[derive(Debug, Clone)]
pub struct Branch {
    pub id: String,
    pub name_key: String,
}

#[derive(Debug, Clone)]
pub struct Skill {
    pub id: String,
    pub branch: usize,
    pub name_key: String,
    pub helps: Vec<Attr>,
}

#[derive(Debug, Clone)]
pub struct ClassDef {
    pub id: String,
    pub branch: usize,
    pub tier: u8,
    pub name_key: String,
    pub desc_key: String,
    /// (skill index, level) pairs.
    pub requires: Vec<(usize, i32)>,
    pub min_rank: usize,
    pub bonus: [i32; Attr::COUNT],
}

#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub name_key: String,
    pub slot: ItemSlot,
    pub two_handed: bool,
    pub bonus: [i32; Attr::COUNT],
    pub price: i64,
}

#[derive(Debug, Clone)]
pub struct Prefab {
    pub id: String,
    pub name_key: String,
    pub desc_key: String,
    pub kind: PrefabKind,
    pub skills: Vec<usize>,
    pub footprint: Vec<(i32, i32)>,
    pub staff_slot: Option<(i32, i32)>,
    pub slots: Vec<(i32, i32)>,
    pub minutes: i64,
    pub cost: i64,
    pub build_minutes: i64,
    pub renown_tier: usize,
    pub height: f32,
}

/// An inclusive-origin rectangle of cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn from(a: [i32; 4]) -> Rect {
        Rect {
            x: a[0],
            y: a[1],
            w: a[2],
            h: a[3],
        }
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        (self.y..self.y + self.h).flat_map(move |y| (self.x..self.x + self.w).map(move |x| (x, y)))
    }
}

#[derive(Debug, Clone)]
pub struct Layout {
    pub width: i32,
    pub height: i32,
    pub door: (i32, i32),
    pub blocked: Vec<Rect>,
    pub east_wall: Rect,
    pub east_openings: Vec<Rect>,
    pub east_region: Rect,
    /// (prefab index, origin).
    pub placements: Vec<(usize, (i32, i32))>,
}

#[derive(Debug, Clone)]
pub struct Leg {
    pub kind: LegKind,
    pub minutes: i64,
    pub attr: Attr,
    pub target: i32,
}

#[derive(Debug, Clone)]
pub struct QuestTemplate {
    pub id: String,
    pub title_key: String,
    pub desc_key: String,
    pub rank: usize,
    pub gold: (i32, i32),
    pub xp: i64,
    pub injury_minutes: i64,
    pub legs: Vec<Leg>,
    /// (item index, percent).
    pub loot: Vec<(usize, i32)>,
}

#[derive(Debug, Clone)]
pub struct Category {
    pub id: String,
    pub verb_key: String,
    pub targets: Vec<String>,
    pub legs: Vec<(LegKind, Attr)>,
}

/// Sealed, validated content. Immutable for the life of a world.
#[derive(Debug, Clone)]
pub struct Content {
    pub rules: Rules,
    pub offsets: Offsets,
    pub consequence_objective: ConsequenceWeights,
    pub consequence_other: ConsequenceWeights,
    pub ranks: Vec<Rank>,
    pub demo_rank_cap: usize,
    pub attributes: Vec<AttributeDef>,
    pub given_names: Vec<String>,
    pub family_names: Vec<String>,
    pub roles: Vec<Role>,
    pub shift_lengths: Vec<u8>,
    pub starting_adventurers: Vec<usize>,
    pub starting_staff: Vec<StartingStaff>,
    pub branches: Vec<Branch>,
    pub skill_levels: Vec<i32>,
    pub skills: Vec<Skill>,
    pub skill_xp_per_leg: i32,
    pub aptitude_min: i32,
    pub aptitude_max: i32,
    pub aspiration_bonus_percent: i32,
    pub classes: Vec<ClassDef>,
    pub items: Vec<Item>,
    pub prefabs: Vec<Prefab>,
    pub layout: Layout,
    pub quests: Vec<QuestTemplate>,
    pub locations: Vec<String>,
    pub categories: Vec<Category>,
    pub loc: Loc,
    /// A hash over every id in registry order. Saves record it; a save made against different
    /// content is refused rather than silently misread.
    pub fingerprint: u64,
}

impl Content {
    pub fn rank_offset(&self, kind: LegKind) -> i32 {
        match kind {
            LegKind::Objective => self.offsets.objective,
            LegKind::Encounter => self.offsets.encounter,
            LegKind::Travel => self.offsets.travel,
            LegKind::Return => self.offsets.ret,
        }
    }
    pub fn consequence(&self, kind: LegKind) -> ConsequenceWeights {
        if kind == LegKind::Objective {
            self.consequence_objective
        } else {
            self.consequence_other
        }
    }
    pub fn prefab_index(&self, id: &str) -> Option<usize> {
        self.prefabs.iter().position(|p| p.id == id)
    }
    pub fn item_index(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|p| p.id == id)
    }
    pub fn role_for_station(&self, kind: PrefabKind) -> Option<usize> {
        self.roles.iter().position(|r| r.station == kind)
    }
    pub fn base_class_for_branch(&self, branch: usize) -> usize {
        self.classes
            .iter()
            .position(|c| c.branch == branch && c.tier == 0)
            .unwrap_or(0)
    }
    /// The renown tier a renown total sits in.
    pub fn renown_tier(&self, renown: i64) -> usize {
        self.rules
            .renown_tiers
            .iter()
            .rposition(|t| renown >= *t)
            .unwrap_or(0)
    }
}

struct Checker<'a> {
    loc: &'a Loc,
    missing_keys: BTreeSet<String>,
}

impl Checker<'_> {
    fn key(&mut self, key: &str) {
        if !self.loc.has(key) {
            self.missing_keys.insert(key.to_string());
        }
    }
}

fn index_of(ids: &[String], id: &str, what: &str, file: &str) -> Result<usize, GameError> {
    ids.iter().position(|x| x == id).ok_or_else(|| {
        GameError::content(format!(
            "{file}: unknown {what} id '{id}'. Valid ids: {}",
            ids.join(", ")
        ))
    })
}

fn no_duplicates(ids: &[String], what: &str, file: &str) -> Result<(), GameError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(id) {
            return Err(GameError::content(format!(
                "{file}: duplicate {what} id '{id}'"
            )));
        }
    }
    Ok(())
}

fn bonus_array(map: &BTreeMap<Attr, i32>) -> [i32; Attr::COUNT] {
    let mut out = [0; Attr::COUNT];
    for (a, v) in map {
        out[a.index()] = *v;
    }
    out
}

pub fn resolve(raw: RawContent, loc: Loc) -> Result<Content, GameError> {
    let mut chk = Checker {
        loc: &loc,
        missing_keys: BTreeSet::new(),
    };
    let err = |m: String| Err(GameError::content(m));

    // Ranks.
    let ranks: Vec<Rank> = raw
        .ranks
        .ranks
        .iter()
        .map(|r| Rank {
            id: r.id.clone(),
            letter: r.letter.clone(),
            attribute_floor: r.attribute_floor,
            target_base: r.target_base,
            hire_baseline: r.hire_baseline,
            modifier_cap: r.modifier_cap,
            injury_minutes: r.injury_minutes,
            promote_quests: r.promote_quests,
            promote_xp: r.promote_xp,
            promote_fee: r.promote_fee,
            gold_min: r.gold_min,
            gold_max: r.gold_max,
            xp: r.xp,
        })
        .collect();
    no_duplicates(
        &ranks.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
        "rank",
        "ranks.yaml",
    )?;
    if raw.ranks.demo_rank_cap >= ranks.len() {
        return err("ranks.yaml: demo_rank_cap is past the top of the ladder".into());
    }
    for r in &ranks {
        if r.gold_min > r.gold_max {
            return err(format!("ranks.yaml: {} gold_min exceeds gold_max", r.id));
        }
    }
    let rules = raw.rules;
    if rules.renown_per_success.len() != ranks.len() {
        return err(format!(
            "rules.yaml: renown_per_success needs one entry per rank ({})",
            ranks.len()
        ));
    }
    if rules.candidate_rank_weights.len() != rules.renown_tiers.len()
        || rules.candidate_rank_weights.iter().any(|w| w.len() != 3)
    {
        return err(
            "rules.yaml: candidate_rank_weights needs one [F, E, D] row per renown tier".into(),
        );
    }
    if rules.renown_goal_tier >= rules.renown_tiers.len() {
        return err("rules.yaml: renown_goal_tier is past the last tier".into());
    }
    if rules.idle_dwell_min >= rules.idle_dwell_max {
        return err("rules.yaml: idle_dwell_min must be below idle_dwell_max".into());
    }
    for (i, _) in rules.renown_tiers.iter().enumerate() {
        chk.key(&format!("renown.tier.{i}"));
    }

    // People.
    let people = raw.people;
    let attributes: Vec<AttributeDef> = people
        .attributes
        .iter()
        .map(|a| {
            chk.key(&a.name_key);
            chk.key(&a.short_key);
            AttributeDef {
                attr: a.id,
                name_key: a.name_key.clone(),
                short_key: a.short_key.clone(),
                min: a.min,
                max: a.max,
            }
        })
        .collect();
    if attributes.len() != Attr::COUNT
        || attributes
            .iter()
            .enumerate()
            .any(|(i, a)| a.attr.index() != i)
    {
        return err(format!(
            "people.yaml: attributes must list all six in sorted-id order: {}",
            Attr::ALL.map(Attr::token).join(", ")
        ));
    }
    if people.given_names.is_empty() || people.family_names.is_empty() {
        return err("people.yaml: name lists may not be empty".into());
    }
    let roles: Vec<Role> = people
        .staff_roles
        .iter()
        .map(|r| {
            chk.key(&r.name_key);
            Role {
                id: r.id.clone(),
                name_key: r.name_key.clone(),
                station: r.station,
                wage_per_hour: r.wage_per_hour,
            }
        })
        .collect();
    let role_ids: Vec<String> = roles.iter().map(|r| r.id.clone()).collect();
    no_duplicates(&role_ids, "role", "people.yaml")?;
    let mut starting_staff = Vec::new();
    for s in &people.starting_roster.staff {
        let role = index_of(&role_ids, &s.role, "role", "people.yaml")?;
        if !people.shift_lengths.contains(&s.length) || s.start_hour > 23 {
            return err(format!(
                "people.yaml: starting staff shift {}h at {} is not a valid template",
                s.length, s.start_hour
            ));
        }
        starting_staff.push(StartingStaff {
            role,
            start_hour: s.start_hour,
            length: s.length,
        });
    }
    for r in &people.starting_roster.adventurers {
        if *r > raw.ranks.demo_rank_cap {
            return err("people.yaml: a starting adventurer is above the demo rank cap".into());
        }
    }

    // Progression.
    let prog = raw.progression;
    let branch_ids: Vec<String> = prog.branches.iter().map(|b| b.id.clone()).collect();
    no_duplicates(&branch_ids, "branch", "progression.yaml")?;
    let branches: Vec<Branch> = prog
        .branches
        .iter()
        .map(|b| {
            chk.key(&b.name_key);
            Branch {
                id: b.id.clone(),
                name_key: b.name_key.clone(),
            }
        })
        .collect();
    let skill_ids: Vec<String> = prog.skills.iter().map(|s| s.id.clone()).collect();
    no_duplicates(&skill_ids, "skill", "progression.yaml")?;
    let mut skills = Vec::new();
    for s in &prog.skills {
        chk.key(&s.name_key);
        skills.push(Skill {
            id: s.id.clone(),
            branch: index_of(&branch_ids, &s.branch, "branch", "progression.yaml")?,
            name_key: s.name_key.clone(),
            helps: s.helps.clone(),
        });
    }
    if prog.skill_levels.windows(2).any(|w| w[0] >= w[1]) {
        return err("progression.yaml: skill_levels must rise strictly".into());
    }
    let class_ids: Vec<String> = prog.classes.iter().map(|c| c.id.clone()).collect();
    no_duplicates(&class_ids, "class", "progression.yaml")?;
    let mut classes = Vec::new();
    for c in &prog.classes {
        chk.key(&c.name_key);
        chk.key(&c.desc_key);
        let mut requires = Vec::new();
        for (skill, level) in &c.requires {
            let s = index_of(&skill_ids, skill, "skill", "progression.yaml")?;
            if *level < 1 || *level as usize > prog.skill_levels.len() {
                return err(format!(
                    "progression.yaml: {} requires {skill} at level {level}, outside 1..{}",
                    c.id,
                    prog.skill_levels.len()
                ));
            }
            requires.push((s, *level));
        }
        if c.min_rank > raw.ranks.demo_rank_cap {
            return err(format!(
                "progression.yaml: {} needs a rank above the demo cap",
                c.id
            ));
        }
        classes.push(ClassDef {
            id: c.id.clone(),
            branch: index_of(&branch_ids, &c.branch, "branch", "progression.yaml")?,
            tier: c.tier,
            name_key: c.name_key.clone(),
            desc_key: c.desc_key.clone(),
            requires,
            min_rank: c.min_rank,
            bonus: bonus_array(&c.bonus),
        });
    }
    for (b, branch) in branches.iter().enumerate() {
        if !classes.iter().any(|c| c.branch == b && c.tier == 0) {
            return err(format!(
                "progression.yaml: branch {} has no base (tier 0) class",
                branch.id
            ));
        }
    }
    let item_ids: Vec<String> = prog.items.iter().map(|i| i.id.clone()).collect();
    no_duplicates(&item_ids, "item", "progression.yaml")?;
    let items: Vec<Item> = prog
        .items
        .iter()
        .map(|i| {
            chk.key(&i.name_key);
            Item {
                id: i.id.clone(),
                name_key: i.name_key.clone(),
                slot: i.slot,
                two_handed: i.two_handed,
                bonus: bonus_array(&i.bonus),
                price: i.price,
            }
        })
        .collect();
    for i in &items {
        if i.two_handed && i.slot != ItemSlot::MainHand {
            return err(format!(
                "progression.yaml: {} is two-handed but not a main-hand item",
                i.id
            ));
        }
    }
    for s in ItemSlot::ALL {
        chk.key(s.key());
    }

    // Building.
    let b = raw.building;
    let prefab_ids: Vec<String> = b.prefabs.iter().map(|p| p.id.clone()).collect();
    no_duplicates(&prefab_ids, "prefab", "building.yaml")?;
    let mut prefabs = Vec::new();
    for p in &b.prefabs {
        chk.key(&p.name_key);
        chk.key(&p.desc_key);
        let footprint: Vec<(i32, i32)> = p.footprint.iter().map(|c| (c[0], c[1])).collect();
        let slots: Vec<(i32, i32)> = p.slots.iter().map(|c| (c[0], c[1])).collect();
        let staff_slot = p.staff_slot.map(|c| (c[0], c[1]));
        if footprint.is_empty() {
            return err(format!("building.yaml: {} has an empty footprint", p.id));
        }
        for s in slots.iter().chain(staff_slot.iter()) {
            if footprint.contains(s) {
                return err(format!(
                    "building.yaml: {} has a slot at {:?} inside its own footprint",
                    p.id, s
                ));
            }
        }
        let mut skills_idx = Vec::new();
        for s in &p.skills {
            skills_idx.push(index_of(&skill_ids, s, "skill", "building.yaml")?);
        }
        match p.kind {
            PrefabKind::Training if skills_idx.is_empty() => {
                return err(format!(
                    "building.yaml: training prefab {} trains no skill",
                    p.id
                ));
            }
            PrefabKind::Desk | PrefabKind::Canteen | PrefabKind::Infirmary
                if staff_slot.is_none() =>
            {
                return err(format!(
                    "building.yaml: staffed prefab {} has no staff_slot",
                    p.id
                ));
            }
            _ => {}
        }
        if p.renown_tier >= rules.renown_tiers.len() {
            return err(format!(
                "building.yaml: {} needs a renown tier that does not exist",
                p.id
            ));
        }
        prefabs.push(Prefab {
            id: p.id.clone(),
            name_key: p.name_key.clone(),
            desc_key: p.desc_key.clone(),
            kind: p.kind,
            skills: skills_idx,
            footprint,
            staff_slot,
            slots,
            minutes: p.minutes,
            cost: p.cost,
            build_minutes: p.build_minutes,
            renown_tier: p.renown_tier,
            height: p.height,
        });
    }
    for r in &roles {
        if !prefabs.iter().any(|p| p.kind == r.station) {
            return err(format!("people.yaml: role {} has no station prefab", r.id));
        }
    }
    let l = &b.layout;
    let mut placements = Vec::new();
    for p in &l.placements {
        placements.push((
            index_of(&prefab_ids, &p.prefab, "prefab", "building.yaml")?,
            (p.at[0], p.at[1]),
        ));
    }
    let layout = Layout {
        width: l.width,
        height: l.height,
        door: (l.door[0], l.door[1]),
        blocked: l.blocked.iter().map(|r| Rect::from(*r)).collect(),
        east_wall: Rect::from(l.east_wing.wall),
        east_openings: l
            .east_wing
            .openings
            .iter()
            .map(|r| Rect::from(*r))
            .collect(),
        east_region: Rect::from(l.east_wing.region),
        placements,
    };
    if !placements_have(&layout, &prefabs, PrefabKind::Desk)
        || !placements_have(&layout, &prefabs, PrefabKind::Board)
    {
        return err("building.yaml: the starting hall needs a desk and a quest board".into());
    }

    // Quests.
    let q = raw.quests;
    let quest_ids: Vec<String> = q.curated.iter().map(|q| q.id.clone()).collect();
    no_duplicates(&quest_ids, "quest", "quests.yaml")?;
    let mut quests = Vec::new();
    for t in &q.curated {
        chk.key(&t.title_key);
        chk.key(&t.desc_key);
        let legs: Vec<Leg> = t
            .legs
            .iter()
            .map(|l| Leg {
                kind: l.kind,
                minutes: l.minutes,
                attr: l.attr,
                target: l.target,
            })
            .collect();
        if legs.iter().filter(|l| l.kind == LegKind::Objective).count() != 1 {
            return err(format!(
                "quests.yaml: {} must have exactly one objective leg",
                t.id
            ));
        }
        if legs.last().map(|l| l.kind) != Some(LegKind::Return) {
            return err(format!("quests.yaml: {} must end with a return leg", t.id));
        }
        if legs.iter().any(|l| l.minutes <= 0) {
            return err(format!("quests.yaml: {} has a leg of no duration", t.id));
        }
        if t.rank > raw.ranks.demo_rank_cap {
            return err(format!("quests.yaml: {} is above the demo rank cap", t.id));
        }
        if t.gold[0] > t.gold[1] {
            return err(format!("quests.yaml: {} gold range is backwards", t.id));
        }
        let mut loot = Vec::new();
        for l in &t.loot {
            loot.push((
                index_of(&item_ids, &l.item, "item", "quests.yaml")?,
                l.percent,
            ));
        }
        quests.push(QuestTemplate {
            id: t.id.clone(),
            title_key: t.title_key.clone(),
            desc_key: t.desc_key.clone(),
            rank: t.rank,
            gold: (t.gold[0], t.gold[1]),
            xp: t.xp,
            injury_minutes: t.injury_minutes.unwrap_or(rules.injury_default_minutes),
            legs,
            loot,
        });
    }
    for loc_key in &q.procedural.locations {
        chk.key(loc_key);
    }
    let mut categories = Vec::new();
    for c in &q.procedural.categories {
        chk.key(&c.verb_key);
        for t in &c.targets {
            chk.key(t);
        }
        if c.legs.iter().filter(|l| l.0 == LegKind::Objective).count() != 1
            || c.legs.last().map(|l| l.0) != Some(LegKind::Return)
        {
            return err(format!(
                "quests.yaml: procedural category {} needs one objective and a final return",
                c.id
            ));
        }
        categories.push(Category {
            id: c.id.clone(),
            verb_key: c.verb_key.clone(),
            targets: c.targets.clone(),
            legs: c.legs.clone(),
        });
    }

    if !chk.missing_keys.is_empty() {
        return err(format!(
            "lang/en.yaml: missing localisation keys: {}",
            chk.missing_keys.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    let mut fp = String::new();
    for ids in [
        &ranks.iter().map(|r| r.id.clone()).collect::<Vec<_>>(),
        &role_ids,
        &branch_ids,
        &skill_ids,
        &class_ids,
        &item_ids,
        &prefab_ids,
        &quest_ids,
    ] {
        for id in ids {
            fp.push_str(id);
            fp.push('|');
        }
        fp.push('#');
    }

    Ok(Content {
        rules,
        offsets: raw.ranks.offsets,
        consequence_objective: raw.ranks.consequences.objective,
        consequence_other: raw.ranks.consequences.other,
        ranks,
        demo_rank_cap: raw.ranks.demo_rank_cap,
        attributes,
        given_names: people.given_names,
        family_names: people.family_names,
        roles,
        shift_lengths: people.shift_lengths,
        starting_adventurers: people.starting_roster.adventurers,
        starting_staff,
        branches,
        skill_levels: prog.skill_levels,
        skills,
        skill_xp_per_leg: prog.skill_xp_per_leg,
        aptitude_min: prog.aptitude_min,
        aptitude_max: prog.aptitude_max,
        aspiration_bonus_percent: prog.aspiration_bonus_percent,
        classes,
        items,
        prefabs,
        layout,
        quests,
        locations: q.procedural.locations,
        categories,
        fingerprint: fnv1a64(&fp),
        loc,
    })
}

fn placements_have(layout: &Layout, prefabs: &[Prefab], kind: PrefabKind) -> bool {
    layout
        .placements
        .iter()
        .any(|(p, _)| prefabs[*p].kind == kind)
}
