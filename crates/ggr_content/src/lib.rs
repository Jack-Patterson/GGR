//! Content: everything the game is made of that is data rather than rules.
//!
//! YAML is authored under `content/`, embedded into the binary at compile time (so a shipped
//! build cannot lose a file or trip over a path separator), parsed into raw serde types,
//! validated, and resolved into a sealed [`Content`] whose cross references are indices. An
//! unknown or typo'd id is an error naming the valid set; `cargo test` runs the validator over
//! the shipped content, which is the demo's "fails the build" contract.

mod loc;
mod raw;
mod resolve;

use std::collections::BTreeMap;

pub use loc::Loc;
pub use resolve::*;

use ggr_core::GameError;

/// The six attributes, in V2's sorted-id order (the order generation draws them in).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Attr {
    Agility,
    Arcana,
    Might,
    Perception,
    Presence,
    Vitality,
}

impl Attr {
    pub const ALL: [Attr; 6] = [
        Attr::Agility,
        Attr::Arcana,
        Attr::Might,
        Attr::Perception,
        Attr::Presence,
        Attr::Vitality,
    ];
    pub const COUNT: usize = 6;

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn token(self) -> &'static str {
        match self {
            Attr::Agility => "agility",
            Attr::Arcana => "arcana",
            Attr::Might => "might",
            Attr::Perception => "perception",
            Attr::Presence => "presence",
            Attr::Vitality => "vitality",
        }
    }
}

/// One file of content: a name for error messages and its text.
pub struct Source<'a> {
    pub name: &'a str,
    pub text: &'a str,
}

/// The shipped content, embedded at compile time.
pub const EMBEDDED: [(&str, &str); 7] = [
    ("rules.yaml", include_str!("../../../content/rules.yaml")),
    ("ranks.yaml", include_str!("../../../content/ranks.yaml")),
    ("people.yaml", include_str!("../../../content/people.yaml")),
    (
        "progression.yaml",
        include_str!("../../../content/progression.yaml"),
    ),
    (
        "building.yaml",
        include_str!("../../../content/building.yaml"),
    ),
    ("quests.yaml", include_str!("../../../content/quests.yaml")),
    (
        "lang/en.yaml",
        include_str!("../../../content/lang/en.yaml"),
    ),
];

impl Content {
    /// Loads, validates and seals the shipped content.
    pub fn load_embedded() -> Result<Content, GameError> {
        let mut files = BTreeMap::new();
        for (name, text) in EMBEDDED {
            files.insert(name.to_string(), text.to_string());
        }
        Content::load(&files)
    }

    /// Loads from an explicit set of files keyed by the names in [`EMBEDDED`]. Tests use this
    /// to feed in broken content and assert the validator's message.
    pub fn load(files: &BTreeMap<String, String>) -> Result<Content, GameError> {
        let get = |name: &str| {
            files
                .get(name)
                .map(String::as_str)
                .ok_or_else(|| GameError::content(format!("missing content file {name}")))
        };
        let raw = raw::RawContent::parse(
            get("rules.yaml")?,
            get("ranks.yaml")?,
            get("people.yaml")?,
            get("progression.yaml")?,
            get("building.yaml")?,
            get("quests.yaml")?,
        )?;
        let loc = Loc::parse(get("lang/en.yaml")?)?;
        resolve::resolve(raw, loc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_content_validates() {
        let c = Content::load_embedded().expect("shipped content must validate");
        assert_eq!(c.attributes.len(), Attr::COUNT);
        assert!(c.quests.len() >= 14);
        assert!(c.prefabs.len() >= 10);
    }

    #[test]
    fn attribute_order_is_pinned_to_the_enum() {
        let c = Content::load_embedded().unwrap();
        for (i, a) in c.attributes.iter().enumerate() {
            assert_eq!(
                a.attr.index(),
                i,
                "attribute {} out of order",
                a.attr.token()
            );
        }
    }

    fn files_with(name: &str, from: &str, to: &str) -> BTreeMap<String, String> {
        let mut files = BTreeMap::new();
        for (n, t) in EMBEDDED {
            let mut t = t.to_string();
            if n == name {
                assert!(t.contains(from), "probe text {from} not found in {name}");
                t = t.replacen(from, to, 1);
            }
            files.insert(n.to_string(), t);
        }
        files
    }

    #[test]
    fn typo_in_a_loot_id_names_the_valid_set() {
        let files = files_with("quests.yaml", "item.tallow_lantern", "item.tallow_lanturn");
        let err = Content::load(&files).unwrap_err().to_string();
        assert!(err.contains("item.tallow_lanturn"), "{err}");
        assert!(err.contains("item.iron_greatsword"), "{err}");
    }

    #[test]
    fn typo_in_a_skill_requirement_fails() {
        let files = files_with(
            "progression.yaml",
            "requires: { skill.blade: 3 }",
            "requires: { skill.blaed: 3 }",
        );
        let err = Content::load(&files).unwrap_err().to_string();
        assert!(err.contains("skill.blaed"), "{err}");
    }

    #[test]
    fn missing_localisation_key_fails() {
        let files = files_with(
            "quests.yaml",
            "title_key: quest.lost_cat",
            "title_key: quest.lost_dog",
        );
        let err = Content::load(&files).unwrap_err().to_string();
        assert!(err.contains("quest.lost_dog"), "{err}");
    }

    #[test]
    fn unknown_attribute_fails() {
        let files = files_with(
            "quests.yaml",
            "attr: might, target: 9",
            "attr: mihgt, target: 9",
        );
        let err = Content::load(&files).unwrap_err().to_string();
        assert!(err.contains("mihgt"), "{err}");
    }

    #[test]
    fn quest_without_objective_fails() {
        let files = files_with(
            "quests.yaml",
            "- { kind: objective, minutes: 60, attr: perception, target: 8 }",
            "- { kind: encounter, minutes: 60, attr: perception, target: 8 }",
        );
        let err = Content::load(&files).unwrap_err().to_string();
        assert!(err.contains("objective"), "{err}");
    }

    #[test]
    fn duplicate_id_fails() {
        let files = files_with(
            "quests.yaml",
            "id: quest.lost_cat.f",
            "id: quest.rat_cellar.f",
        );
        let err = Content::load(&files).unwrap_err().to_string();
        assert!(err.contains("duplicate"), "{err}");
    }

    #[test]
    fn slot_inside_a_footprint_fails() {
        let files = files_with(
            "building.yaml",
            "footprint: [[0, 0]], slots: [[0, 1]], minutes: 5,",
            "footprint: [[0, 0]], slots: [[0, 0]], minutes: 5,",
        );
        let err = Content::load(&files).unwrap_err().to_string();
        assert!(err.contains("footprint"), "{err}");
    }
}
