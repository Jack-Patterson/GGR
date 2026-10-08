//! Progression: skills raised by quests and training, aspired classes with a visible "what's
//! missing" list, rank promotion offered through the desk, and equipment. Every bonus feeding a
//! check is printable, and items + skills + class together are capped by the rank's
//! modifier cap so a geared adventurer never out-ranks the ladder.

use ggr_content::{Attr, ItemSlot};

use crate::commands::Refusal;
use crate::sched::Handler;
use crate::types::*;
use crate::World;

/// Everything that goes into one check, for the quest report and the character sheet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CheckBreakdown {
    pub attr_value: i32,
    pub items: i32,
    pub skills: i32,
    pub class: i32,
    pub cap: i32,
    pub gear_capped: i32,
}

/// One requirement row on the aspired-class panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassProgress {
    /// Skill index, or None for the rank requirement.
    pub skill: Option<usize>,
    pub needed: i32,
    pub have: i32,
}

impl ClassProgress {
    pub fn met(&self) -> bool {
        self.have >= self.needed
    }
}

impl World {
    pub fn skill_level(&self, id: CharId, skill: usize) -> i32 {
        let Some(adv) = &self.s.chars[id as usize].adv else {
            return 0;
        };
        let xp = adv.skill_xp.get(skill).copied().unwrap_or(0);
        self.content
            .skill_levels
            .iter()
            .take_while(|t| xp >= **t)
            .count() as i32
    }

    pub fn check_breakdown(&self, id: CharId, attr: Attr) -> CheckBreakdown {
        let c = &self.s.chars[id as usize];
        let mut b = CheckBreakdown {
            attr_value: c.attrs[attr.index()],
            ..Default::default()
        };
        let Some(adv) = &c.adv else {
            return b;
        };
        for item in adv.equipment.iter().flatten() {
            b.items += self.content.items[*item as usize].bonus[attr.index()];
        }
        let best_skill = self
            .content
            .skills
            .iter()
            .enumerate()
            .filter(|(_, s)| s.helps.contains(&attr))
            .map(|(i, _)| self.skill_level(id, i))
            .max()
            .unwrap_or(0);
        b.skills = best_skill / 2;
        let class = &self.content.classes[adv.class as usize];
        b.class = class.bonus[attr.index()];
        // A tier-1 class keeps its base class's bonus too.
        if class.tier > 0 {
            let base = self.content.base_class_for_branch(class.branch);
            b.class += self.content.classes[base].bonus[attr.index()];
        }
        b.cap = self.content.ranks[c.rank as usize].modifier_cap;
        b.gear_capped = (b.items + b.skills + b.class).min(b.cap);
        b
    }

    pub(crate) fn award_skill_xp(&mut self, id: CharId, skill: usize, base: i32) {
        let before = self.skill_level(id, skill);
        let bonus = self.content.aspiration_bonus_percent;
        let branch = self.content.skills[skill].branch;
        let aspired_skills = self.aspired_skills(id);
        let c = &mut self.s.chars[id as usize];
        let Some(adv) = &mut c.adv else { return };
        let mut xp = base * adv.aptitude[branch] / 100;
        if aspired_skills.is_some_and(|s| s.contains(&skill)) {
            xp += xp * bonus / 100;
        }
        adv.skill_xp[skill] += xp.max(1);
        let after = self.skill_level(id, skill);
        if after > before {
            self.emit(SimEvent::SkillUp {
                character: id,
                skill: skill as u8,
                level: after,
            });
        }
    }

    pub(crate) fn award_leg_skill_xp(&mut self, id: CharId, attr: Attr, passed: bool) {
        let base = self.content.skill_xp_per_leg / if passed { 1 } else { 2 };
        let skills: Vec<usize> = self
            .content
            .skills
            .iter()
            .enumerate()
            .filter(|(_, s)| s.helps.contains(&attr))
            .map(|(i, _)| i)
            .collect();
        for s in skills {
            self.award_skill_xp(id, s, base);
        }
    }

    /// The skills an adventurer's aspired class asks for, if they aspire to one.
    pub(crate) fn aspired_skills(&self, id: CharId) -> Option<Vec<usize>> {
        let adv = self.s.chars[id as usize].adv.as_ref()?;
        let class = &self.content.classes[adv.aspiration? as usize];
        Some(class.requires.iter().map(|(s, _)| *s).collect())
    }

    /// The aspired class's requirements against what the adventurer has: the "what's missing"
    /// list the character sheet shows.
    pub fn class_progress(&self, id: CharId, class: usize) -> Vec<ClassProgress> {
        let c = &self.s.chars[id as usize];
        let def = &self.content.classes[class];
        let mut out: Vec<ClassProgress> = def
            .requires
            .iter()
            .map(|(s, lvl)| ClassProgress {
                skill: Some(*s),
                needed: *lvl,
                have: self.skill_level(id, *s),
            })
            .collect();
        if def.min_rank > 0 {
            out.push(ClassProgress {
                skill: None,
                needed: def.min_rank as i32,
                have: i32::from(c.rank),
            });
        }
        out
    }

    /// Promotes into the aspired class once every requirement is met.
    pub(crate) fn check_class_progress(&mut self, id: CharId) {
        let Some(adv) = &self.s.chars[id as usize].adv else {
            return;
        };
        let Some(asp) = adv.aspiration else { return };
        if adv.class == asp {
            return;
        }
        if self
            .class_progress(id, asp as usize)
            .iter()
            .all(ClassProgress::met)
        {
            let adv = self.s.chars[id as usize].adv.as_mut().unwrap();
            adv.class = asp;
            adv.aspiration = None;
            self.emit(SimEvent::ClassAttained {
                character: id,
                class: asp,
            });
        }
    }

    pub(crate) fn set_aspiration(&mut self, id: CharId, class: Option<u8>) -> Result<(), Refusal> {
        let c = self
            .s
            .chars
            .get(id as usize)
            .ok_or(Refusal::UnknownCharacter)?;
        if !c.alive() || c.adv.is_none() {
            return Err(Refusal::UnknownCharacter);
        }
        if let Some(k) = class {
            let def = self
                .content
                .classes
                .get(k as usize)
                .ok_or(Refusal::UnknownClass)?;
            if def.tier == 0 {
                return Err(Refusal::UnknownClass);
            }
        }
        self.s.chars[id as usize].adv.as_mut().unwrap().aspiration = class;
        if class.is_some() {
            self.complete_objective(Objective::SetAspiration);
            self.check_class_progress(id);
        }
        Ok(())
    }

    /// After a successful return: offer promotion if the adventurer has earned it.
    pub(crate) fn check_promotion(&mut self, id: CharId) {
        let c = &self.s.chars[id as usize];
        let Some(adv) = &c.adv else { return };
        if !c.alive() || c.rank as usize >= self.content.demo_rank_cap {
            return;
        }
        if self.s.promotions.iter().any(|p| p.character == id) {
            return;
        }
        let r = &self.content.ranks[c.rank as usize];
        if adv.quests_at_rank < r.promote_quests || adv.xp < r.promote_xp {
            return;
        }
        let now = self.s.minute;
        let expires = now + self.content.rules.candidate_window_minutes;
        self.s.promotions.push(PromotionOffer {
            character: id,
            to_rank: c.rank + 1,
            fee: r.promote_fee,
            expires,
        });
        self.s
            .sched
            .schedule(now, expires, Handler::PromotionExpiry, u64::from(id));
        self.emit(SimEvent::PromotionOffered { character: id });
    }

    pub(crate) fn on_promotion_expiry(&mut self, payload: u64) {
        let id = payload as CharId;
        let now = self.s.minute;
        // Lapsed: dropped now, offered again after their next success.
        self.s
            .promotions
            .retain(|p| !(p.character == id && p.expires <= now));
    }

    pub(crate) fn accept_promotion(&mut self, id: CharId) -> Result<(), Refusal> {
        let Some(pos) = self.s.promotions.iter().position(|p| p.character == id) else {
            return Err(Refusal::NoOffer);
        };
        if !self.desk_can_sign() {
            return Err(Refusal::DeskUnmanned);
        }
        let offer = self.s.promotions[pos].clone();
        self.debit(offer.fee, GoldReason::Promotion)?;
        self.s.promotions.remove(pos);
        let c = &mut self.s.chars[id as usize];
        let step = self.content.ranks[offer.to_rank as usize].attribute_floor
            - self.content.ranks[c.rank as usize].attribute_floor;
        c.rank = offer.to_rank;
        for a in &mut c.attrs {
            *a += step;
        }
        if let Some(adv) = &mut c.adv {
            adv.quests_at_rank = 0;
        }
        self.s.stats.promotions += 1;
        self.complete_objective(Objective::Promote);
        self.emit(SimEvent::Promoted {
            character: id,
            rank: offer.to_rank,
        });
        self.check_class_progress(id);
        Ok(())
    }

    pub(crate) fn decline_promotion(&mut self, id: CharId) -> Result<(), Refusal> {
        let before = self.s.promotions.len();
        self.s.promotions.retain(|p| p.character != id);
        if before == self.s.promotions.len() {
            return Err(Refusal::NoOffer);
        }
        Ok(())
    }

    pub(crate) fn equip(&mut self, id: CharId, item: usize) -> Result<(), Refusal> {
        let c = self
            .s
            .chars
            .get(id as usize)
            .ok_or(Refusal::UnknownCharacter)?;
        if !c.alive() || c.adv.is_none() {
            return Err(Refusal::UnknownCharacter);
        }
        if c.state == CharState::OffMap(OffMapReason::Quest) {
            return Err(Refusal::AwayOnQuest);
        }
        let def = self
            .content
            .items
            .get(item)
            .ok_or(Refusal::UnknownItem)?
            .clone();
        if self.s.guild.stash[item] == 0 {
            return Err(Refusal::NotInStash);
        }
        // Whatever the new item displaces goes back to the stash.
        let mut displaced = Vec::new();
        {
            let adv = self.s.chars[id as usize].adv.as_mut().unwrap();
            let slot = def.slot.index();
            if let Some(old) = adv.equipment[slot].take() {
                displaced.push(old);
            }
            if def.two_handed {
                if let Some(old) = adv.equipment[ItemSlot::OffHand.index()].take() {
                    displaced.push(old);
                }
            }
            if def.slot == ItemSlot::OffHand {
                if let Some(main) = adv.equipment[ItemSlot::MainHand.index()] {
                    if self.content.items[main as usize].two_handed {
                        adv.equipment[ItemSlot::MainHand.index()] = None;
                        displaced.push(main);
                    }
                }
            }
            adv.equipment[slot] = Some(item as u16);
        }
        self.s.guild.stash[item] -= 1;
        for d in displaced {
            self.s.guild.stash[d as usize] += 1;
        }
        self.complete_objective(Objective::EquipItem);
        Ok(())
    }

    pub(crate) fn unequip(&mut self, id: CharId, slot: ItemSlot) -> Result<(), Refusal> {
        let c = self
            .s
            .chars
            .get_mut(id as usize)
            .ok_or(Refusal::UnknownCharacter)?;
        if c.state == CharState::OffMap(OffMapReason::Quest) {
            return Err(Refusal::AwayOnQuest);
        }
        let adv = c.adv.as_mut().ok_or(Refusal::UnknownCharacter)?;
        let item = adv.equipment[slot.index()]
            .take()
            .ok_or(Refusal::NothingThere)?;
        self.s.guild.stash[item as usize] += 1;
        Ok(())
    }

    pub(crate) fn buy(&mut self, item: usize) -> Result<(), Refusal> {
        let price = self
            .content
            .items
            .get(item)
            .ok_or(Refusal::UnknownItem)?
            .price;
        if price <= 0 {
            return Err(Refusal::NotForSale);
        }
        self.debit(price, GoldReason::Purchase)?;
        self.s.guild.stash[item] += 1;
        Ok(())
    }
}
