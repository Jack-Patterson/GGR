//! Commands in. Every player action is a validated command: it either commits whole or is
//! refused with a reason the UI can show, and nothing changes.

use ggr_content::ItemSlot;

use crate::types::*;
use crate::World;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Hire {
        candidate: u32,
    },
    Decline {
        candidate: u32,
    },
    AcceptPromotion {
        character: CharId,
    },
    DeclinePromotion {
        character: CharId,
    },
    SetShift {
        character: CharId,
        start_hour: u8,
        length: u8,
    },
    AssignWorkstation {
        character: CharId,
        instance: Option<InstId>,
    },
    SetAspiration {
        character: CharId,
        class: Option<u8>,
    },
    Equip {
        character: CharId,
        item: usize,
    },
    Unequip {
        character: CharId,
        slot: ItemSlot,
    },
    Buy {
        item: usize,
    },
    Place {
        prefab: usize,
        x: i32,
        y: i32,
    },
    Demolish {
        instance: InstId,
    },
    OpenEastWing,
    Dismiss {
        character: CharId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOk {
    Done,
    Hired(CharId),
    Placed(InstId),
}

/// Why a command was refused. Each has a localisation key for the UI's notice line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    UnknownCandidate,
    CandidateGone,
    DeskUnmanned,
    InsufficientGold,
    UnknownCharacter,
    NotStaff,
    BadShift,
    WrongStation,
    UnknownClass,
    UnknownItem,
    NotInStash,
    NothingThere,
    NotForSale,
    AwayOnQuest,
    NoOffer,
    UnknownPrefab,
    NeedsRenown,
    Blocked,
    WingLocked,
    Occupied,
    BlocksDoor,
    SomeoneThere,
    CutsOffAccess,
    UnknownInstance,
    LastOfItsKind,
    AlreadyOpen,
}

impl Refusal {
    pub fn key(self) -> &'static str {
        match self {
            Refusal::UnknownCandidate | Refusal::CandidateGone => "refusal.candidate_gone",
            Refusal::DeskUnmanned => "refusal.desk_unmanned",
            Refusal::InsufficientGold => "refusal.gold",
            Refusal::UnknownCharacter => "refusal.character",
            Refusal::NotStaff => "refusal.not_staff",
            Refusal::BadShift => "refusal.bad_shift",
            Refusal::WrongStation => "refusal.wrong_station",
            Refusal::UnknownClass => "refusal.class",
            Refusal::UnknownItem | Refusal::NotInStash => "refusal.not_in_stash",
            Refusal::NothingThere => "refusal.nothing_there",
            Refusal::NotForSale => "refusal.not_for_sale",
            Refusal::AwayOnQuest => "refusal.away",
            Refusal::NoOffer => "refusal.no_offer",
            Refusal::UnknownPrefab => "refusal.prefab",
            Refusal::NeedsRenown => "refusal.renown",
            Refusal::Blocked => "refusal.blocked",
            Refusal::WingLocked => "refusal.wing_locked",
            Refusal::Occupied => "refusal.occupied",
            Refusal::BlocksDoor => "refusal.door",
            Refusal::SomeoneThere => "refusal.someone_there",
            Refusal::CutsOffAccess => "refusal.access",
            Refusal::UnknownInstance => "refusal.instance",
            Refusal::LastOfItsKind => "refusal.last_of_kind",
            Refusal::AlreadyOpen => "refusal.already_open",
        }
    }

    pub const ALL: [Refusal; 26] = [
        Refusal::UnknownCandidate,
        Refusal::CandidateGone,
        Refusal::DeskUnmanned,
        Refusal::InsufficientGold,
        Refusal::UnknownCharacter,
        Refusal::NotStaff,
        Refusal::BadShift,
        Refusal::WrongStation,
        Refusal::UnknownClass,
        Refusal::UnknownItem,
        Refusal::NotInStash,
        Refusal::NothingThere,
        Refusal::NotForSale,
        Refusal::AwayOnQuest,
        Refusal::NoOffer,
        Refusal::UnknownPrefab,
        Refusal::NeedsRenown,
        Refusal::Blocked,
        Refusal::WingLocked,
        Refusal::Occupied,
        Refusal::BlocksDoor,
        Refusal::SomeoneThere,
        Refusal::CutsOffAccess,
        Refusal::UnknownInstance,
        Refusal::LastOfItsKind,
        Refusal::AlreadyOpen,
    ];
}

impl World {
    /// Executes a command. Refused commands change nothing.
    pub fn execute(&mut self, cmd: Command) -> Result<CommandOk, Refusal> {
        match cmd {
            Command::Hire { candidate } => self.hire(candidate).map(CommandOk::Hired),
            Command::Decline { candidate } => self.decline(candidate).map(|_| CommandOk::Done),
            Command::AcceptPromotion { character } => {
                self.accept_promotion(character).map(|_| CommandOk::Done)
            }
            Command::DeclinePromotion { character } => {
                self.decline_promotion(character).map(|_| CommandOk::Done)
            }
            Command::SetShift {
                character,
                start_hour,
                length,
            } => self
                .set_shift(character, start_hour, length)
                .map(|_| CommandOk::Done),
            Command::AssignWorkstation {
                character,
                instance,
            } => self
                .assign_workstation(character, instance)
                .map(|_| CommandOk::Done),
            Command::SetAspiration { character, class } => self
                .set_aspiration(character, class)
                .map(|_| CommandOk::Done),
            Command::Equip { character, item } => {
                self.equip(character, item).map(|_| CommandOk::Done)
            }
            Command::Unequip { character, slot } => {
                self.unequip(character, slot).map(|_| CommandOk::Done)
            }
            Command::Buy { item } => self.buy(item).map(|_| CommandOk::Done),
            Command::Place { prefab, x, y } => self.place(prefab, (x, y)).map(CommandOk::Placed),
            Command::Demolish { instance } => self.demolish(instance).map(|_| CommandOk::Done),
            Command::OpenEastWing => self.open_east_wing().map(|_| CommandOk::Done),
            Command::Dismiss { character } => self.dismiss(character).map(|_| CommandOk::Done),
        }
    }
}
