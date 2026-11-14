//! Rules an organization sets for its people (SPEC §5.9, "per-org
//! policies"). Pure: the settings and how several of them combine.

use serde::{Deserialize, Serialize};

#[allow(unused_imports)] // used by the code `string_enum!` generates
use crate::{Error, Result};

crate::model::string_enum!(
    /// What managers see of the entries of the people they manage.
    ManagerVisibility { Full => "full", HideNotes => "hide_notes", TotalsOnly => "totals_only" }
);
crate::model::string_enum!(
    /// Whether colleagues see who is online.
    PresenceVisibility { Org => "org", Off => "off" }
);

#[allow(clippy::derivable_impls)] // the enums come from `string_enum!`
impl Default for ManagerVisibility {
    fn default() -> Self {
        Self::Full
    }
}

#[allow(clippy::derivable_impls)]
impl Default for PresenceVisibility {
    fn default() -> Self {
        Self::Org
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase", default)]
pub struct PolicySettings {
    /// Finished entries must name a project.
    pub require_project: bool,
    /// Finished entries must have at least one tag.
    pub require_tags: bool,
    pub manager_visibility: ManagerVisibility,
    pub presence_visibility: PresenceVisibility,
    /// Target per week, for comparing with the tracked time (overtime).
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub weekly_target_minutes: Option<i64>,
    /// Suggested idle time before auto-pause; people may choose less.
    #[cfg_attr(feature = "specta", specta(type = Option<specta_typescript::Number>))]
    pub auto_pause_idle_minutes: Option<i64>,
}

impl PolicySettings {
    /// The stricter of two policies: what one of them requires, the result requires;
    /// the smaller target and idle time win.
    pub fn stricter(&self, other: &Self) -> Self {
        let min = |a: Option<i64>, b: Option<i64>| match (a, b) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        let manager = |v: ManagerVisibility| match v {
            ManagerVisibility::Full => 0,
            ManagerVisibility::HideNotes => 1,
            ManagerVisibility::TotalsOnly => 2,
        };
        Self {
            require_project: self.require_project || other.require_project,
            require_tags: self.require_tags || other.require_tags,
            manager_visibility: if manager(other.manager_visibility) > manager(self.manager_visibility) {
                other.manager_visibility
            } else {
                self.manager_visibility
            },
            presence_visibility: if self.presence_visibility == PresenceVisibility::Off
                || other.presence_visibility == PresenceVisibility::Off
            {
                PresenceVisibility::Off
            } else {
                PresenceVisibility::Org
            },
            weekly_target_minutes: min(self.weekly_target_minutes, other.weekly_target_minutes),
            auto_pause_idle_minutes: min(self.auto_pause_idle_minutes, other.auto_pause_idle_minutes),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct OrgPolicy {
    pub id: String,
    /// `None`: the whole organization; otherwise a department like `dev/backend`.
    pub group_path: Option<String>,
    #[serde(flatten)]
    pub settings: PolicySettings,
}

/// The policy that applies to a person: the organization's, made stricter by
/// that of every department they are in.
pub fn effective(policies: &[OrgPolicy], my_groups: &[String]) -> PolicySettings {
    policies
        .iter()
        .filter(|p| p.group_path.as_ref().is_none_or(|g| my_groups.contains(g)))
        .fold(PolicySettings::default(), |acc, p| acc.stricter(&p.settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(group: Option<&str>, settings: PolicySettings) -> OrgPolicy {
        OrgPolicy {
            id: "x".into(),
            group_path: group.map(Into::into),
            settings,
        }
    }

    #[test]
    fn departments_make_the_organizations_policy_stricter_never_looser() {
        let org = policy(
            None,
            PolicySettings {
                require_project: true,
                weekly_target_minutes: Some(2400),
                ..Default::default()
            },
        );
        let dev = policy(
            Some("dev"),
            PolicySettings {
                require_tags: true,
                manager_visibility: ManagerVisibility::HideNotes,
                weekly_target_minutes: Some(2100),
                presence_visibility: PresenceVisibility::Off,
                ..Default::default()
            },
        );
        let ops = policy(
            Some("ops"),
            PolicySettings {
                require_tags: true,
                ..Default::default()
            },
        );
        let all = [org, dev, ops];

        let anna = effective(&all, &["dev".into()]);
        assert!(anna.require_project && anna.require_tags);
        assert_eq!(anna.manager_visibility, ManagerVisibility::HideNotes);
        assert_eq!(anna.weekly_target_minutes, Some(2100));
        assert_eq!(anna.presence_visibility, PresenceVisibility::Off);

        // Without a department only the organization's rules apply.
        let ben = effective(&all, &[]);
        assert!(ben.require_project && !ben.require_tags);
        assert_eq!(
            (ben.manager_visibility, ben.weekly_target_minutes),
            (ManagerVisibility::Full, Some(2400))
        );
        assert_eq!(effective(&[], &["dev".into()]), PolicySettings::default());
    }

    #[test]
    fn the_flat_json_is_what_syncs() {
        let p = policy(
            Some("dev"),
            PolicySettings {
                require_tags: true,
                ..Default::default()
            },
        );
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["groupPath"], "dev");
        assert_eq!(json["requireTags"], true);
        assert_eq!(json["managerVisibility"], "full");
        let back: OrgPolicy = serde_json::from_value(json).unwrap();
        assert_eq!(back, p);
    }
}
