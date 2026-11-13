//! Fields that only make sense together.
//!
//! Sync merges field by field, newest write wins. That is wrong for fields
//! that must stay consistent with each other: a country profile and its region
//! (`DE` with an Austrian region), a start and an end (an end before the
//! start breaks the table's checks), a date range and its half-day flag. Two
//! devices editing different members of such a group would produce a mix that
//! neither of them made.
//!
//! So these fields travel as one *group field*: an object holding all of the
//! members. The outbox turns the members of a payload into the group on the
//! way out; applying writes the members back to their columns.

use serde_json::{Map, Value};

/// (table, group field name, member fields).
pub const GROUPS: &[(&str, &str, &[&str])] = &[
    ("jurisdiction_selection", "selection", &["profileId", "region"]),
    ("absence", "span", &["startDate", "endDate", "halfDay"]),
    ("time_entry", "span", &["startAt", "endAt", "tz"]),
    ("pause", "span", &["startAt", "endAt"]),
];

fn groups_of(table: &str) -> impl Iterator<Item = (&'static str, &'static [&'static str])> + '_ {
    GROUPS
        .iter()
        .filter(move |(t, _, _)| *t == table)
        .map(|(_, name, members)| (*name, *members))
}

/// Replaces the members of each group in `payload` with the group field.
/// A group is built only when at least one member is present; missing
/// members are `null`.
pub fn group_payload(table: &str, payload: &mut Value) {
    let Some(object) = payload.as_object_mut() else {
        return;
    };
    for (name, members) in groups_of(table) {
        if !members.iter().any(|m| object.contains_key(*m)) {
            continue;
        }
        let mut group = Map::new();
        for member in members {
            group.insert(
                (*member).to_string(),
                object.remove(*member).unwrap_or(Value::Null),
            );
        }
        object.insert(name.to_string(), Value::Object(group));
    }
}

/// The member fields of a group field, or `None` if `field` is not a group of `table`.
pub fn expand(table: &str, field: &str, value: &Value) -> Option<Vec<(String, Value)>> {
    let (_, members) = groups_of(table).find(|(name, _)| *name == field)?;
    let object = value.as_object()?;
    Some(
        members
            .iter()
            .map(|m| ((*m).to_string(), object.get(*m).cloned().unwrap_or(Value::Null)))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn members_become_one_field_and_back() {
        let mut payload = json!({"profileId": "AT", "region": "AT-9", "other": 1});
        group_payload("jurisdiction_selection", &mut payload);
        assert_eq!(
            payload,
            json!({"selection": {"profileId": "AT", "region": "AT-9"}, "other": 1})
        );

        let parts = expand("jurisdiction_selection", "selection", &payload["selection"]).unwrap();
        assert_eq!(
            parts,
            vec![
                ("profileId".to_string(), json!("AT")),
                ("region".to_string(), json!("AT-9"))
            ]
        );
    }

    #[test]
    fn a_missing_member_is_null_and_other_tables_are_untouched() {
        let mut entry = json!({"startAt": 1, "tz": "UTC", "note": "x"});
        group_payload("time_entry", &mut entry);
        assert_eq!(
            entry,
            json!({"span": {"startAt": 1, "endAt": null, "tz": "UTC"}, "note": "x"})
        );

        let mut project = json!({"startAt": 1});
        group_payload("project", &mut project);
        assert_eq!(project, json!({"startAt": 1}));
        assert!(expand("project", "span", &json!({})).is_none());
        assert!(expand("time_entry", "note", &json!("x")).is_none());
    }
}
