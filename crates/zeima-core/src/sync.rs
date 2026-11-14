//! Sync merge (SPEC §6): field-level last-write-wins by HLC, plus the rule
//! overrides the server enforces.
//!
//! This is a pure state-based merge: a row's state keeps, for every field,
//! the value and the HLC of the write that set it, plus the HLC of the latest
//! delete. Merging a change takes the newer HLC per field and the newer
//! delete, so it is commutative and idempotent: devices that see the same
//! changes in any order end in the same state. Whether a row is live is
//! derived from that state (`RowState::is_live`).
//!
//! The same algorithm is implemented in TypeScript in
//! `server/src/sync/merge.ts`. Both run the vectors in
//! `packages/shared-fixtures/sync/merge.json`; change one, change the other.
//!
//! HLC strings are fixed-width (see `hlc.rs`), so comparing them as strings
//! compares the timestamps, and the node id breaks exact ties.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One field with the HLC of the write that set it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Versioned {
    pub v: Value,
    pub h: String,
}

/// What is known about one row.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RowState {
    pub fields: BTreeMap<String, Versioned>,
    /// HLC of the latest delete.
    pub deleted: Option<String>,
}

impl RowState {
    /// A row is live unless it was deleted and no field was written after.
    pub fn is_live(&self) -> bool {
        match &self.deleted {
            None => true,
            Some(d) => self.fields.values().any(|f| f.h.as_str() > d.as_str()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Upsert,
    Delete,
}

/// A change from a device. `fields` holds only the fields that changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub table: String,
    pub row_id: String,
    pub op: Op,
    pub hlc: String,
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
}

/// Who sends the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Actor {
    #[serde(default)]
    pub manager: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The entry is locked and the sender is not a manager.
    Locked,
    /// Only managers change the `locked` flag.
    ForbiddenField,
    /// The server's organization rules: not allowed for this person or table.
    Forbidden,
    ForbiddenTable,
    /// The organization's policy asks for a project or tags on an entry.
    PolicyProject,
    PolicyTags,
    /// Another meeting has the room at that time.
    RoomTaken,
}

impl Reason {
    /// What the person is told when the server refuses their change.
    pub fn advice(self) -> &'static str {
        match self {
            Reason::Locked => "The entry is locked; only a manager can change it.",
            Reason::ForbiddenField => "Only managers change this part of an entry.",
            Reason::Forbidden | Reason::ForbiddenTable => {
                "Your role in the organization does not allow this change."
            }
            Reason::PolicyProject => "The organization's rules ask for a project on every entry.",
            Reason::PolicyTags => "The organization's rules ask for tags on every entry.",
            Reason::RoomTaken => "The room is booked at that time. Pick another room or time.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Outcome {
    /// The change altered the state.
    Applied,
    /// Nothing to do: everything in it was already superseded.
    Ignored,
    /// A rule refused it; the state is unchanged.
    Rejected { reason: Reason },
}

const TIME_ENTRY: &str = "time_entry";
const LOCKED: &str = "locked";

fn is_locked(state: &RowState) -> bool {
    state.fields.get(LOCKED).is_some_and(|f| f.v == Value::Bool(true))
}

/// Rule overrides (SPEC §6.2): locked time entries are changed by managers
/// only, and only managers change the lock itself.
fn check_rules(state: &RowState, change: &Change, actor: Actor) -> Option<Reason> {
    if change.table != TIME_ENTRY || actor.manager {
        return None;
    }
    if is_locked(state) {
        return Some(Reason::Locked);
    }
    if let Some(new) = change.fields.get(LOCKED) {
        if *new != Value::Bool(is_locked(state)) {
            return Some(Reason::ForbiddenField);
        }
    }
    None
}

/// Merges `change` into `state` (`None`: the row is unknown).
pub fn merge(state: Option<&RowState>, change: &Change, actor: Actor) -> (RowState, Outcome) {
    let mut next = state.cloned().unwrap_or_default();
    if let Some(reason) = check_rules(&next, change, actor) {
        return (state.cloned().unwrap_or_default(), Outcome::Rejected { reason });
    }
    let mut changed = false;
    match change.op {
        Op::Delete => {
            if next.deleted.as_deref().is_none_or(|d| change.hlc.as_str() > d) {
                next.deleted = Some(change.hlc.clone());
                changed = true;
            }
        }
        Op::Upsert => {
            for (name, value) in &change.fields {
                let newer = next
                    .fields
                    .get(name)
                    .is_none_or(|f| change.hlc.as_str() > f.h.as_str());
                if newer {
                    next.fields.insert(
                        name.clone(),
                        Versioned {
                            v: value.clone(),
                            h: change.hlc.clone(),
                        },
                    );
                    changed = true;
                }
            }
        }
    }
    (
        next,
        if changed {
            Outcome::Applied
        } else {
            Outcome::Ignored
        },
    )
}

/// Merges two states of the same row: per field the newer write, and the
/// newer delete. Same result as applying one side's writes as changes.
pub fn merge_state(a: &RowState, b: &RowState) -> RowState {
    let mut out = a.clone();
    for (name, theirs) in &b.fields {
        if out.fields.get(name).is_none_or(|mine| theirs.h > mine.h) {
            out.fields.insert(name.clone(), theirs.clone());
        }
    }
    if b.deleted
        .as_ref()
        .is_some_and(|d| out.deleted.as_ref().is_none_or(|mine| d > mine))
    {
        out.deleted = b.deleted.clone();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTORS: &str = include_str!("../../../packages/shared-fixtures/sync/merge.json");

    #[derive(Deserialize)]
    struct Vectors {
        cases: Vec<Case>,
        convergence: Vec<Convergence>,
    }

    #[derive(Deserialize)]
    struct Case {
        name: String,
        initial: Option<RowState>,
        #[serde(default)]
        actor: Actor,
        changes: Vec<Change>,
        expect: Expect,
    }

    #[derive(Deserialize)]
    struct Expect {
        state: RowState,
        results: Vec<Outcome>,
    }

    #[derive(Deserialize)]
    struct Convergence {
        name: String,
        changes: Vec<Change>,
    }

    fn run(initial: Option<RowState>, changes: &[Change], actor: Actor) -> (Option<RowState>, Vec<Outcome>) {
        let mut state = initial;
        let mut results = Vec::new();
        for change in changes {
            let (next, outcome) = merge(state.as_ref(), change, actor);
            // A rejected change leaves an unknown row unknown.
            if !matches!(outcome, Outcome::Rejected { .. }) {
                state = Some(next);
            }
            results.push(outcome);
        }
        (state, results)
    }

    fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
        if items.len() <= 1 {
            return vec![items.to_vec()];
        }
        let mut all = Vec::new();
        for i in 0..items.len() {
            let mut rest = items.to_vec();
            let head = rest.remove(i);
            for mut tail in permutations(&rest) {
                tail.insert(0, head.clone());
                all.push(tail);
            }
        }
        all
    }

    #[test]
    fn shared_vectors() {
        let vectors: Vectors = serde_json::from_str(VECTORS).expect("merge.json");
        assert!(!vectors.cases.is_empty());
        for case in vectors.cases {
            let (state, results) = run(case.initial, &case.changes, case.actor);
            assert_eq!(results, case.expect.results, "{}: outcomes", case.name);
            assert_eq!(
                state.unwrap_or_default(),
                case.expect.state,
                "{}: state",
                case.name
            );
        }
    }

    #[test]
    fn changes_converge_in_any_order_and_when_repeated() {
        let vectors: Vectors = serde_json::from_str(VECTORS).expect("merge.json");
        assert!(!vectors.convergence.is_empty());
        for case in vectors.convergence {
            let mut finals = permutations(&case.changes).into_iter().map(|order| {
                let (state, _) = run(None, &order, Actor { manager: true });
                // Applying everything a second time changes nothing.
                let (again, results) = run(state.clone(), &order, Actor { manager: true });
                assert!(
                    results.iter().all(|r| *r == Outcome::Ignored),
                    "{}: replay changed the state",
                    case.name
                );
                assert_eq!(state, again, "{}: replay", case.name);
                state.unwrap_or_default()
            });
            let first = finals.next().unwrap();
            for other in finals {
                assert_eq!(first, other, "{}: order matters", case.name);
            }
        }
    }

    #[test]
    fn merging_states_equals_applying_the_changes() {
        let vectors: Vectors = serde_json::from_str(VECTORS).expect("merge.json");
        for case in vectors.convergence {
            // Two replicas each see part of the changes; merging their states
            // gives the state of one replica that saw everything.
            let (left, right) = case.changes.split_at(case.changes.len() / 2);
            let manager = Actor { manager: true };
            let a = run(None, left, manager).0.unwrap_or_default();
            let b = run(None, right, manager).0.unwrap_or_default();
            let all = run(None, &case.changes, manager).0.unwrap_or_default();
            assert_eq!(merge_state(&a, &b), all, "{}", case.name);
            assert_eq!(merge_state(&b, &a), all, "{}: commutative", case.name);
        }
    }

    #[test]
    fn a_row_is_live_until_deleted_and_not_written_after() {
        let mut row = RowState::default();
        assert!(row.is_live());
        row.deleted = Some("000000000000020-000000-a".into());
        assert!(!row.is_live());
        row.fields.insert(
            "name".into(),
            Versioned {
                v: Value::from("x"),
                h: "000000000000030-000000-a".into(),
            },
        );
        assert!(row.is_live());
    }
}
