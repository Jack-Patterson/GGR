//! Building: placing and demolishing prefabs, construction, and opening the East Wing. Every
//! placement rule lives here in command validation, so an illegal build cannot be committed
//! whatever the UI shows.

use ggr_content::PrefabKind;

use crate::commands::Refusal;
use crate::sched::{pack, Handler};
use crate::types::*;
use crate::World;

impl World {
    pub(crate) fn build_starting_hall(&mut self) {
        let placements = self.content().layout.placements.clone();
        for (prefab, origin) in placements {
            let id = self.add_instance(prefab as u16, origin, InstanceStatus::Ready, 0);
            let cost = self.content().prefabs[prefab].cost;
            self.s.instances[id as usize].paid = cost;
        }
    }

    fn add_instance(
        &mut self,
        prefab: u16,
        origin: Cell,
        status: InstanceStatus,
        paid: i64,
    ) -> InstId {
        let id = self.s.instances.len() as InstId;
        let p = &self.content().prefabs[prefab as usize];
        let mut slots = Vec::new();
        if let Some((dx, dy)) = p.staff_slot {
            slots.push(Slot {
                cell: (origin.0 + dx, origin.1 + dy),
                staff: true,
                holder: None,
                occupied: false,
            });
        }
        for (dx, dy) in &p.slots {
            slots.push(Slot {
                cell: (origin.0 + dx, origin.1 + dy),
                staff: false,
                holder: None,
                occupied: false,
            });
        }
        self.s.instances.push(Instance {
            id,
            prefab,
            origin,
            status,
            slots,
            paid,
        });
        self.stamp_instance(id);
        id
    }

    /// Writes an instance's footprint and slots into the grid.
    pub(crate) fn stamp_instance(&mut self, id: InstId) {
        let inst = &self.s.instances[id as usize];
        if inst.status == InstanceStatus::Demolished {
            return;
        }
        let p = &self.content.prefabs[inst.prefab as usize];
        for (dx, dy) in &p.footprint {
            let (x, y) = (inst.origin.0 + dx, inst.origin.1 + dy);
            self.grid.set(x, y, crate::grid::CELL_FOOTPRINT);
            self.grid.set_owner(x, y, Some(id));
        }
        for s in &inst.slots {
            self.grid.set(s.cell.0, s.cell.1, crate::grid::CELL_SLOT);
            self.grid.set_owner(s.cell.0, s.cell.1, Some(id));
        }
    }

    /// Rebuilds the derived grid from layout, wing state and instances (after a load).
    pub(crate) fn rebuild_grid(&mut self) {
        self.grid = crate::grid::Grid::from_layout(&self.content.layout);
        if self.s.east_wing_open {
            let l = self.content.layout.clone();
            self.grid.open_east_wing(&l);
        }
        for i in 0..self.s.instances.len() {
            self.stamp_instance(i as InstId);
        }
    }

    pub fn prefab_unlocked(&self, prefab: usize) -> bool {
        self.content().prefabs[prefab].renown_tier <= self.s.guild.renown_tier
    }

    /// Checks a placement without committing it. The same function the command uses, so the
    /// build-mode ghost and the commit can never disagree.
    pub fn validate_placement(&mut self, prefab: usize, origin: Cell) -> Result<(), Refusal> {
        let Some(p) = self.content.prefabs.get(prefab) else {
            return Err(Refusal::UnknownPrefab);
        };
        if p.renown_tier > self.s.guild.renown_tier {
            return Err(Refusal::NeedsRenown);
        }
        if self.s.guild.gold < p.cost {
            return Err(Refusal::InsufficientGold);
        }
        let g = &self.grid;
        let fp: Vec<Cell> = p
            .footprint
            .iter()
            .map(|(dx, dy)| (origin.0 + dx, origin.1 + dy))
            .collect();
        let mut slots: Vec<Cell> = p
            .slots
            .iter()
            .chain(p.staff_slot.iter())
            .map(|(dx, dy)| (origin.0 + dx, origin.1 + dy))
            .collect();
        for &(x, y) in fp.iter().chain(slots.iter()) {
            if !g.in_bounds(x, y) || g.has(x, y, crate::grid::CELL_WALL) {
                return Err(Refusal::Blocked);
            }
            if g.has(x, y, crate::grid::CELL_LOCKED) {
                return Err(Refusal::WingLocked);
            }
            if g.has(x, y, crate::grid::CELL_FOOTPRINT | crate::grid::CELL_SLOT) {
                return Err(Refusal::Occupied);
            }
            if g.has(x, y, crate::grid::CELL_DOOR) {
                return Err(Refusal::BlocksDoor);
            }
        }
        // Nobody may be standing where the footprint goes down.
        for c in &self.s.chars {
            if c.on_map() && c.state != CharState::Travel && fp.contains(&c.cell) {
                return Err(Refusal::SomeoneThere);
            }
        }
        // Every slot in the hall, and this prefab's own, must still be reachable from the door.
        let mut trial = self.grid.clone();
        for &(x, y) in &fp {
            trial.set(x, y, crate::grid::CELL_FOOTPRINT);
        }
        for inst in &self.s.instances {
            if inst.status != InstanceStatus::Demolished {
                slots.extend(inst.slots.iter().map(|s| s.cell));
            }
        }
        if !trial.all_reachable(trial.door, &slots, &mut self.scratch) {
            return Err(Refusal::CutsOffAccess);
        }
        Ok(())
    }

    pub(crate) fn place(&mut self, prefab: usize, origin: Cell) -> Result<InstId, Refusal> {
        self.validate_placement(prefab, origin)?;
        let (cost, minutes) = {
            let p = &self.content.prefabs[prefab];
            (p.cost, p.build_minutes.max(1))
        };
        self.debit(cost, GoldReason::Construction)?;
        let now = self.s.minute;
        let id = self.add_instance(
            prefab as u16,
            origin,
            InstanceStatus::UnderConstruction {
                done_at: now + minutes,
            },
            cost,
        );
        self.s
            .sched
            .schedule(now, now + minutes, Handler::ConstructionDone, u64::from(id));
        self.emit(SimEvent::ConstructionStarted { instance: id });
        Ok(id)
    }

    pub(crate) fn on_construction_done(&mut self, payload: u64) {
        let id = payload as InstId;
        let Some(inst) = self.s.instances.get_mut(id as usize) else {
            return;
        };
        if !matches!(inst.status, InstanceStatus::UnderConstruction { .. }) {
            self.s.stats.stale_events += 1;
            return;
        }
        inst.status = InstanceStatus::Ready;
        self.s.stats.built += 1;
        let kind = self.content.prefabs[inst.prefab as usize].kind;
        if kind == PrefabKind::Canteen {
            self.complete_objective(Objective::BuildCanteen);
        }
        self.emit(SimEvent::Built { instance: id });
        // Off-duty staff whose station just appeared are picked up by the next hourly check;
        // on-duty staff idling for want of a station are nudged now.
        let now = self.s.minute;
        for i in 0..self.s.chars.len() {
            let c = &self.s.chars[i];
            if c.is_staff() && c.state == CharState::Idle {
                let serial = c.serial;
                self.s
                    .sched
                    .schedule(now, now + 1, Handler::IdleDecision, pack(serial, i as u32));
            }
        }
    }

    pub(crate) fn demolish(&mut self, id: InstId) -> Result<(), Refusal> {
        let Some(inst) = self.s.instances.get(id as usize) else {
            return Err(Refusal::UnknownInstance);
        };
        if inst.status == InstanceStatus::Demolished {
            return Err(Refusal::UnknownInstance);
        }
        let kind = self.content.prefabs[inst.prefab as usize].kind;
        if matches!(kind, PrefabKind::Desk | PrefabKind::Board) {
            let others = self.s.instances.iter().any(|o| {
                o.id != id && o.is_ready() && self.content.prefabs[o.prefab as usize].kind == kind
            });
            if !others {
                return Err(Refusal::LastOfItsKind);
            }
        }
        let refund = match inst.status {
            InstanceStatus::UnderConstruction { .. } => inst.paid,
            _ => inst.paid * self.content.rules.demolish_refund_percent / 100,
        };
        // Everyone using or heading for it resolves through the ordinary rules: walkers finish
        // their walk and decide again, occupants stand up and go idle.
        let holders: Vec<(CharId, bool)> = inst
            .slots
            .iter()
            .filter_map(|s| s.holder.map(|h| (h, s.occupied)))
            .collect();
        for (h, _) in &holders {
            self.cancel_hold(*h, id);
        }
        let inst = &mut self.s.instances[id as usize];
        inst.status = InstanceStatus::Demolished;
        let p = &self.content.prefabs[inst.prefab as usize];
        for (dx, dy) in &p.footprint {
            let (x, y) = (inst.origin.0 + dx, inst.origin.1 + dy);
            self.grid.clear(x, y, crate::grid::CELL_FOOTPRINT);
            self.grid.set_owner(x, y, None);
        }
        for s in &inst.slots {
            self.grid.clear(s.cell.0, s.cell.1, crate::grid::CELL_SLOT);
            self.grid.set_owner(s.cell.0, s.cell.1, None);
        }
        for c in &mut self.s.chars {
            if let Some(st) = &mut c.staff {
                if st.workstation == Some(id) {
                    st.workstation = None;
                }
            }
        }
        if refund > 0 {
            self.credit(refund, GoldReason::Refund);
        }
        self.s.stats.demolished += 1;
        self.emit(SimEvent::Demolished { instance: id });
        Ok(())
    }

    pub(crate) fn open_east_wing(&mut self) -> Result<(), Refusal> {
        if self.s.east_wing_open {
            return Err(Refusal::AlreadyOpen);
        }
        if self.s.guild.renown_tier < self.content.rules.east_wing_renown_tier {
            return Err(Refusal::NeedsRenown);
        }
        self.debit(self.content.rules.east_wing_cost, GoldReason::Expansion)?;
        self.s.east_wing_open = true;
        let l = self.content.layout.clone();
        self.grid.open_east_wing(&l);
        self.complete_objective(Objective::OpenEastWing);
        self.emit(SimEvent::EastWingOpened);
        Ok(())
    }

    /// Instances of a kind that are ready to use.
    pub(crate) fn ready_of_kind(&self, kind: PrefabKind) -> impl Iterator<Item = &Instance> {
        self.s
            .instances
            .iter()
            .filter(move |i| i.is_ready() && self.content.prefabs[i.prefab as usize].kind == kind)
    }

    pub fn kind_of(&self, inst: InstId) -> PrefabKind {
        self.content.prefabs[self.s.instances[inst as usize].prefab as usize].kind
    }

    pub fn decor_count(&self) -> i32 {
        self.ready_of_kind(PrefabKind::Decor).count() as i32
    }
}
