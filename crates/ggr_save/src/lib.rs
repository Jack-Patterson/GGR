//! Persistence: the versioned envelope around the sim's sections, atomic writes with rolling
//! backups, and the migration registry.
//!
//! - Each section is versioned on its own. Migrations step one version at a time, chained,
//!   never skipping; each ships with a committed fixture that must load forever after.
//! - A section this build does not know (a mod's, a newer build's) rides through load and save
//!   untouched.
//! - A write goes to a temporary file, is flushed to disk, and only then renamed over the
//!   slot; the previous save is rotated into numbered backups first. A crash mid-write leaves
//!   the last good save where it was.
//! - All paths are built with `Path::join`; nothing here ever spells a separator.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ggr_content::Content;
use ggr_core::{GameError, BUILD_VERSION};
use ggr_sim::World;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FORMAT: &str = "guildmasters-seat-save";
pub const FORMAT_VERSION: u32 = 1;
pub const BACKUPS: usize = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SectionEnvelope {
    pub version: u32,
    pub data: Value,
}

/// The file as written. The summary fields duplicate a little sim state so the load menu can
/// describe a slot without deserialising the world.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Envelope {
    pub format: String,
    pub format_version: u32,
    pub build: String,
    pub summary: SaveSummary,
    pub sections: BTreeMap<String, SectionEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SaveSummary {
    pub day: i64,
    pub minute: i64,
    pub gold: i64,
    pub roster: u32,
    pub renown: i64,
    pub label: String,
}

/// A one-step section migration: data at `from` becomes data at `from + 1`.
pub struct Migration {
    pub section: &'static str,
    pub from: u32,
    pub apply: fn(Value) -> Result<Value, GameError>,
}

/// The registry. Empty while every section is at version 1; the test module registers a
/// synthetic chain to prove the machinery.
pub fn migrations() -> Vec<Migration> {
    Vec::new()
}

pub fn envelope_from_world(world: &World, label: &str) -> Envelope {
    let sections = world
        .to_sections()
        .into_iter()
        .map(|(k, (version, data))| (k, SectionEnvelope { version, data }))
        .collect();
    Envelope {
        format: FORMAT.to_string(),
        format_version: FORMAT_VERSION,
        build: BUILD_VERSION.to_string(),
        summary: SaveSummary {
            day: world.day(),
            minute: world.minute(),
            gold: world.guild().gold,
            roster: world.characters().iter().filter(|c| c.on_roster()).count() as u32,
            renown: world.guild().renown,
            label: label.to_string(),
        },
        sections,
    }
}

/// Upgrades every section to the version this build reads, one step at a time.
pub fn migrate(env: &mut Envelope, registry: &[Migration]) -> Result<(), GameError> {
    for (name, current) in ggr_sim::SECTION_VERSIONS {
        let Some(sec) = env.sections.get_mut(*name) else {
            continue;
        };
        if sec.version > *current {
            return Err(GameError::save(format!(
                "section '{name}' is version {}, newer than this build reads ({current})",
                sec.version
            )));
        }
        while sec.version < *current {
            let step = registry
                .iter()
                .find(|m| m.section == *name && m.from == sec.version)
                .ok_or_else(|| {
                    GameError::save(format!(
                        "no migration for section '{name}' from version {}",
                        sec.version
                    ))
                })?;
            sec.data = (step.apply)(std::mem::take(&mut sec.data))?;
            sec.version += 1;
        }
    }
    Ok(())
}

pub fn world_from_envelope(
    content: Arc<Content>,
    mut env: Envelope,
    registry: &[Migration],
) -> Result<World, GameError> {
    if env.format != FORMAT {
        return Err(GameError::save("not a Guildmaster's Seat save"));
    }
    if env.format_version != FORMAT_VERSION {
        return Err(GameError::save(format!(
            "save format {} is not readable by this build",
            env.format_version
        )));
    }
    migrate(&mut env, registry)?;
    let sections = env
        .sections
        .into_iter()
        .map(|(k, s)| (k, (s.version, s.data)))
        .collect();
    World::from_sections(content, &sections)
}

pub fn to_json(env: &Envelope) -> String {
    serde_json::to_string_pretty(env).expect("envelope serialises")
}

pub fn from_json(text: &str) -> Result<Envelope, GameError> {
    serde_json::from_str(text).map_err(|e| GameError::save(format!("unreadable save: {e}")))
}

fn io(e: std::io::Error, what: &str, path: &Path) -> GameError {
    GameError::save(format!("{what} {}: {e}", path.display()))
}

fn backup_path(path: &Path, n: usize) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(format!(".bak{n}"));
    path.with_file_name(name)
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Writes `text` to `path` atomically, rotating up to [`BACKUPS`] previous versions.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), GameError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io(e, "cannot create", dir))?;
    }
    let tmp = temp_path(path);
    {
        let mut f = fs::File::create(&tmp).map_err(|e| io(e, "cannot create", &tmp))?;
        f.write_all(text.as_bytes())
            .map_err(|e| io(e, "cannot write", &tmp))?;
        f.sync_all().map_err(|e| io(e, "cannot flush", &tmp))?;
    }
    if path.exists() {
        for n in (1..BACKUPS).rev() {
            let from = backup_path(path, n);
            if from.exists() {
                // Rename over an existing file is atomic on Linux and replaces on Windows.
                fs::rename(&from, backup_path(path, n + 1))
                    .map_err(|e| io(e, "cannot rotate", &from))?;
            }
        }
        fs::copy(path, backup_path(path, 1)).map_err(|e| io(e, "cannot back up", path))?;
    }
    fs::rename(&tmp, path).map_err(|e| io(e, "cannot commit", path))?;
    Ok(())
}

pub fn save_world(path: &Path, world: &World, label: &str) -> Result<(), GameError> {
    write_atomic(path, &to_json(&envelope_from_world(world, label)))
}

pub fn read_envelope(path: &Path) -> Result<Envelope, GameError> {
    let text = fs::read_to_string(path).map_err(|e| io(e, "cannot read", path))?;
    from_json(&text)
}

/// Loads a slot, falling back through its backups if the newest copy is unreadable.
pub fn load_world(path: &Path, content: Arc<Content>) -> Result<World, GameError> {
    let mut first_err = None;
    let candidates =
        std::iter::once(path.to_path_buf()).chain((1..=BACKUPS).map(|n| backup_path(path, n)));
    for p in candidates {
        if !p.exists() {
            continue;
        }
        match read_envelope(&p)
            .and_then(|env| world_from_envelope(content.clone(), env, &migrations()))
        {
            Ok(w) => return Ok(w),
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    Err(first_err.unwrap_or_else(|| GameError::save(format!("no save at {}", path.display()))))
}

/// The save directory and slot naming. The directory is passed in (the game resolves the
/// platform's user-data folder); tests use a temporary one.
#[derive(Debug, Clone)]
pub struct SaveSlots {
    pub dir: PathBuf,
}

impl SaveSlots {
    pub const SLOTS: usize = 3;

    pub fn slot_path(&self, n: usize) -> PathBuf {
        self.dir.join(format!("slot{n}.json"))
    }

    pub fn autosave_path(&self) -> PathBuf {
        self.dir.join("autosave.json")
    }

    pub fn summary(&self, path: &Path) -> Option<SaveSummary> {
        read_envelope(path).ok().map(|e| e.summary)
    }
}

#[cfg(test)]
mod tests;
