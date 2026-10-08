//! The save-section boundary. The sim knows how to turn itself into named, independently
//! versioned sections of plain JSON and back; it knows nothing about files, envelopes,
//! backups or migrations — that is the persistence crate's job.

use std::collections::BTreeMap;
use std::sync::Arc;

use ggr_content::Content;
use ggr_core::GameError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::types::*;
use crate::World;

/// Name and current version of every section the sim owns.
pub const SECTIONS: [(&str, u32); 11] = [
    ("clock", 1),
    ("rng", 1),
    ("scheduler", 1),
    ("world", 1),
    ("characters", 1),
    ("quests", 1),
    ("guild", 1),
    ("candidates", 1),
    ("objectives", 1),
    ("stats", 1),
    ("meta", 1),
];

pub type Sections = BTreeMap<String, (u32, Value)>;

fn to_value<T: Serialize>(v: &T) -> Value {
    serde_json::to_value(v).expect("sim state is always serialisable")
}

fn from_value<T: DeserializeOwned>(sections: &Sections, name: &str) -> Result<T, GameError> {
    let (_, v) = sections
        .get(name)
        .ok_or_else(|| GameError::save(format!("missing section '{name}'")))?;
    serde_json::from_value(v.clone())
        .map_err(|e| GameError::save(format!("section '{name}' is malformed: {e}")))
}

#[derive(Serialize, serde::Deserialize)]
struct WorldSection {
    instances: Vec<Instance>,
    east_wing_open: bool,
    audit: SlotAudit,
}

#[derive(Serialize, serde::Deserialize)]
struct CandidateSection {
    candidates: Vec<Candidate>,
    promotions: Vec<PromotionOffer>,
    next_candidate_id: u32,
}

#[derive(Serialize, serde::Deserialize)]
struct MetaSection {
    content_fingerprint: u64,
}

impl World {
    /// Every section, the sim's own and the foreign ones it is carrying, by name.
    pub fn to_sections(&self) -> Sections {
        let s = &self.s;
        let mut out = Sections::new();
        let mut put = |name: &str, v: Value| {
            let version = SECTIONS.iter().find(|(n, _)| *n == name).map_or(1, |x| x.1);
            out.insert(name.to_string(), (version, v));
        };
        put("clock", serde_json::json!({ "minute": s.minute }));
        put("rng", to_value(&s.rng));
        put("scheduler", to_value(&s.sched));
        put(
            "world",
            to_value(&WorldSection {
                instances: s.instances.clone(),
                east_wing_open: s.east_wing_open,
                audit: s.audit.clone(),
            }),
        );
        put("characters", to_value(&s.chars));
        put("quests", to_value(&s.quests));
        put("guild", to_value(&s.guild));
        put(
            "candidates",
            to_value(&CandidateSection {
                candidates: s.candidates.clone(),
                promotions: s.promotions.clone(),
                next_candidate_id: s.next_candidate_id,
            }),
        );
        put("objectives", to_value(&s.objectives));
        put("stats", to_value(&s.stats));
        put(
            "meta",
            to_value(&MetaSection {
                content_fingerprint: self.content.fingerprint,
            }),
        );
        for (name, sec) in &s.foreign_sections {
            out.insert(name.clone(), sec.clone());
        }
        out
    }

    /// Rebuilds a world from sections (already migrated to current versions). Sections this
    /// build does not know are kept and written back out untouched.
    pub fn from_sections(content: Arc<Content>, sections: &Sections) -> Result<World, GameError> {
        for (name, version) in SECTIONS {
            match sections.get(name) {
                None => return Err(GameError::save(format!("missing section '{name}'"))),
                Some((v, _)) if *v != version => {
                    return Err(GameError::save(format!(
                        "section '{name}' is version {v}; this build reads {version}"
                    )))
                }
                _ => {}
            }
        }
        let meta: MetaSection = from_value(sections, "meta")?;
        if meta.content_fingerprint != content.fingerprint {
            return Err(GameError::save(
                "this save was made with different game content and cannot be loaded".to_string(),
            ));
        }
        #[derive(serde::Deserialize)]
        struct Clock {
            minute: i64,
        }
        let clock: Clock = from_value(sections, "clock")?;
        let world: WorldSection = from_value(sections, "world")?;
        let cands: CandidateSection = from_value(sections, "candidates")?;
        let foreign = sections
            .iter()
            .filter(|(n, _)| !SECTIONS.iter().any(|(k, _)| k == n))
            .map(|(n, v)| (n.clone(), v.clone()))
            .collect();
        let s = State {
            minute: clock.minute,
            rng: from_value(sections, "rng")?,
            sched: from_value(sections, "scheduler")?,
            instances: world.instances,
            east_wing_open: world.east_wing_open,
            chars: from_value(sections, "characters")?,
            quests: from_value(sections, "quests")?,
            guild: from_value(sections, "guild")?,
            candidates: cands.candidates,
            promotions: cands.promotions,
            objectives: from_value(sections, "objectives")?,
            stats: from_value(sections, "stats")?,
            audit: world.audit,
            next_candidate_id: cands.next_candidate_id,
            foreign_sections: foreign,
        };
        let grid = crate::grid::Grid::from_layout(&content.layout);
        let mut w = World {
            content,
            s,
            grid,
            events: Vec::new(),
            scratch: Default::default(),
        };
        w.rebuild_grid();
        if w.s.sched.has_overdue(w.s.minute) {
            return Err(GameError::save(
                "the scheduler holds an event in the past".to_string(),
            ));
        }
        w.verify_consistency().map_err(GameError::save)?;
        Ok(w)
    }

    /// Stores a section the sim does not own (the view's camera, a mod's data).
    pub fn set_foreign_section(&mut self, name: &str, version: u32, value: Value) {
        assert!(
            !SECTIONS.iter().any(|(n, _)| *n == name),
            "{name} is a sim-owned section"
        );
        self.s
            .foreign_sections
            .insert(name.to_string(), (version, value));
    }

    pub fn foreign_section(&self, name: &str) -> Option<&(u32, Value)> {
        self.s.foreign_sections.get(name)
    }
}
