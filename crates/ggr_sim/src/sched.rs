//! The bucket scheduler: events keyed by due minute, fired in booking order within a minute.
//! Events are never cancelled; an event made stale by a later state change is dropped by its
//! handler's serial guard. A BTreeMap keeps iteration (and so serialisation) deterministic.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Handler ids. The variant order is a save contract: append, never reorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Handler {
    IdleDecision,
    Arrival,
    InteractionEnd,
    QuestGeneration,
    QuestLeg,
    QuestExpiry,
    CandidateArrival,
    CandidateExpiry,
    ConstructionDone,
    ServicePoll,
    PromotionExpiry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledEvent {
    pub handler: Handler,
    pub payload: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scheduler {
    buckets: BTreeMap<i64, Vec<ScheduledEvent>>,
    #[serde(skip)]
    spare: Vec<Vec<ScheduledEvent>>,
}

impl Scheduler {
    /// Books `handler` at `due`, which must be in the future: a handler can never book work
    /// into the minute it is running in.
    pub fn schedule(&mut self, now: i64, due: i64, handler: Handler, payload: u64) {
        assert!(
            due > now,
            "scheduled {handler:?} at minute {due}, not after now ({now})"
        );
        let spare = &mut self.spare;
        self.buckets
            .entry(due)
            .or_insert_with(|| spare.pop().unwrap_or_default())
            .push(ScheduledEvent { handler, payload });
    }

    pub fn take(&mut self, minute: i64) -> Option<Vec<ScheduledEvent>> {
        self.buckets.remove(&minute)
    }

    /// Returns a drained bucket's allocation for reuse, so the tick path stops allocating once
    /// warm.
    pub fn recycle(&mut self, mut bucket: Vec<ScheduledEvent>) {
        bucket.clear();
        if self.spare.len() < 64 {
            self.spare.push(bucket);
        }
    }

    pub fn pending(&self) -> usize {
        self.buckets.values().map(Vec::len).sum()
    }

    /// Any event due at or before `minute` that was never run: always a bug.
    pub fn has_overdue(&self, minute: i64) -> bool {
        self.buckets.keys().next().is_some_and(|k| *k <= minute)
    }

    pub fn iter(&self) -> impl Iterator<Item = (i64, &ScheduledEvent)> {
        self.buckets
            .iter()
            .flat_map(|(m, v)| v.iter().map(move |e| (*m, e)))
    }
}

/// Packs a character id and serial into a payload, V2's `(serial << 32) | index`.
pub fn pack(serial: u32, index: u32) -> u64 {
    (u64::from(serial) << 32) | u64::from(index)
}

pub fn unpack(payload: u64) -> (u32, u32) {
    ((payload >> 32) as u32, payload as u32)
}
