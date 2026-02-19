use rusqlite::Connection;
use std::collections::HashSet;

/// Governance configuration — the trust root.
#[derive(Debug, Clone)]
pub struct GovernanceConfig {
    /// The hardcoded CID of the governance agent.
    pub governance_cid: String,
    /// Network identifier.
    pub network_id: String,
}

/// Whitelist of approved Tier 1 agent CIDs, backed by SQLite.
pub struct AgentWhitelist {
    db: Connection,
}

/// A governance-signed update to the whitelist.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WhitelistUpdate {
    pub action: WhitelistAction,
    pub agent_cid: String,
    pub agent_type: String,
    /// CID of the governance agent that issued this update.
    pub governance_cid: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum WhitelistAction {
    Add,
    Remove,
}

impl AgentWhitelist {
    /// Create a new whitelist backed by the given SQLite database.
    pub fn new(db: Connection) -> Result<Self, rusqlite::Error> {
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_whitelist (
                cid TEXT PRIMARY KEY,
                agent_type TEXT NOT NULL,
                added_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
            );"
        )?;
        Ok(Self { db })
    }

    /// Check if a CID is whitelisted.
    pub fn is_whitelisted(&self, cid: &str) -> bool {
        self.db
            .query_row(
                "SELECT 1 FROM agent_whitelist WHERE cid = ?1",
                [cid],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// Apply a whitelist update.
    pub fn apply_update(&self, update: &WhitelistUpdate) -> Result<(), rusqlite::Error> {
        match update.action {
            WhitelistAction::Add => {
                self.db.execute(
                    "INSERT OR REPLACE INTO agent_whitelist (cid, agent_type) VALUES (?1, ?2)",
                    [&update.agent_cid, &update.agent_type],
                )?;
            }
            WhitelistAction::Remove => {
                self.db.execute(
                    "DELETE FROM agent_whitelist WHERE cid = ?1",
                    [&update.agent_cid],
                )?;
            }
        }
        Ok(())
    }

    /// List all whitelisted CIDs.
    pub fn list(&self) -> Result<HashSet<String>, rusqlite::Error> {
        let mut stmt = self.db.prepare("SELECT cid FROM agent_whitelist")?;
        let cids = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(cids)
    }
}
