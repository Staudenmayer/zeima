//! Organization policies in a shared workspace (see [`crate::policy`]).

use sqlx::Row;

use crate::db::Core;
use crate::policy::{effective, OrgPolicy, PolicySettings};
use crate::{Error, Result};

/// `local_meta`: the full paths of the departments the account is in, as the
/// token lists them (`/orgs/<org>/groups/dev/backend`), set by sync.
pub const ACCOUNT_GROUPS_KEY: &str = "account_groups";

/// The same row for the same scope on every device.
fn policy_id(workspace_id: &str, group: Option<&str>) -> String {
    uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("org_policy|{workspace_id}|{}", group.unwrap_or("")).as_bytes(),
    )
    .to_string()
}

impl Core {
    pub async fn list_policies(&self) -> Result<Vec<OrgPolicy>> {
        let rows = sqlx::query(
            "SELECT id, group_path, settings_json FROM org_policy
             WHERE workspace_id = ? AND deleted_at IS NULL ORDER BY group_path IS NOT NULL, group_path",
        )
        .bind(&self.ctx().workspace_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(|r| {
                let text: String = r.try_get("settings_json")?;
                Ok(OrgPolicy {
                    id: r.try_get("id")?,
                    group_path: r.try_get("group_path")?,
                    settings: serde_json::from_str(&text).unwrap_or_default(),
                })
            })
            .collect()
    }

    /// Sets the rules for the whole organization (`None`) or one department.
    pub async fn save_policy(
        &self,
        group_path: Option<String>,
        settings: PolicySettings,
    ) -> Result<OrgPolicy> {
        if !self.ctx().shared {
            return Err(Error::validation(
                "workspace",
                "policies belong to an organization's workspace",
            ));
        }
        let group_path = group_path
            .map(|g| g.trim().trim_matches('/').to_string())
            .filter(|g| !g.is_empty());
        for minutes in [settings.weekly_target_minutes, settings.auto_pause_idle_minutes]
            .into_iter()
            .flatten()
        {
            if !(1..=10_080).contains(&minutes) {
                return Err(Error::validation("minutes", "must be between 1 and 10080"));
            }
        }
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let id = policy_id(&ws, group_path.as_deref());
        let s = tx.stamp();
        sqlx::query(
            "INSERT INTO org_policy (id, workspace_id, group_path, settings_json, created_at, updated_at, hlc, origin_device)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (id) DO UPDATE SET settings_json = excluded.settings_json, deleted_at = NULL,
                updated_at = excluded.updated_at, hlc = excluded.hlc, origin_device = excluded.origin_device",
        )
        .bind(&id)
        .bind(&ws)
        .bind(&group_path)
        .bind(serde_json::to_string(&settings)?)
        .bind(s.now)
        .bind(s.now)
        .bind(&s.hlc)
        .bind(&s.device)
        .execute(tx.conn())
        .await?;
        let policy = OrgPolicy {
            id: id.clone(),
            group_path,
            settings,
        };
        tx.log_upsert("org_policy", &id, &s, &policy).await?;
        tx.commit().await?;
        Ok(policy)
    }

    pub async fn delete_policy(&self, id: &str) -> Result<()> {
        let mut tx = self.begin().await?;
        let ws = tx.ctx().workspace_id.clone();
        let found: Option<String> = sqlx::query_scalar(
            "SELECT id FROM org_policy WHERE id = ? AND workspace_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .bind(&ws)
        .fetch_optional(tx.conn())
        .await?;
        if found.is_none() {
            return Err(super::not_found("policy", id));
        }
        super::soft_delete(&mut tx, "org_policy", id).await?;
        tx.commit().await
    }

    /// The departments (like `dev/backend`) the account is in, in this workspace's organization.
    pub async fn my_departments(&self) -> Result<Vec<String>> {
        let Some(org): Option<String> = sqlx::query_scalar("SELECT org_id FROM workspace WHERE id = ?")
            .bind(&self.ctx().workspace_id)
            .fetch_optional(self.pool())
            .await?
            .flatten()
        else {
            return Ok(Vec::new());
        };
        let stored = self.get_meta(ACCOUNT_GROUPS_KEY).await?.unwrap_or_default();
        let paths: Vec<String> = serde_json::from_str(&stored).unwrap_or_default();
        let prefix = format!("/orgs/{org}/groups/");
        Ok(paths
            .iter()
            .filter_map(|p| p.strip_prefix(&prefix))
            .map(String::from)
            .collect())
    }

    /// What applies to the account here: the organization's policy, made
    /// stricter by those of its departments. The personal workspace has none.
    pub async fn effective_policy(&self) -> Result<PolicySettings> {
        if !self.ctx().shared {
            return Ok(PolicySettings::default());
        }
        Ok(effective(
            &self.list_policies().await?,
            &self.my_departments().await?,
        ))
    }

    /// Refuses a finished entry that breaks the policy (project or tag required).
    pub(crate) async fn check_entry_policy(&self, project_id: Option<&str>, tag_count: usize) -> Result<()> {
        if !self.ctx().shared {
            return Ok(());
        }
        let policy = self.effective_policy().await?;
        if policy.require_project && project_id.is_none() {
            return Err(Error::validation(
                "projectId",
                "your organization requires a project on every entry",
            ));
        }
        if policy.require_tags && tag_count == 0 {
            return Err(Error::validation(
                "tagIds",
                "your organization requires at least one tag on every entry",
            ));
        }
        Ok(())
    }
}
