use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EddStatus {
    Open,
    InProgress,
    DocumentsRequested,
    UnderReview,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EddCase {
    pub id: Uuid,
    pub user_id: Uuid,
    pub trigger_reason: String,
    pub status: EddStatus,
    pub reviewer: Option<String>,
    pub source_of_funds: Option<String>,
    pub source_of_wealth: Option<String>,
    pub notes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Enhanced Due Diligence case manager.
///
/// Tracks EDD investigations from initial trigger through approval or
/// rejection.  Cases are keyed by their own UUID, not by user ID, so a single
/// user can have multiple simultaneous investigations.
pub struct EddManager {
    /// Cases indexed by case ID.
    cases: HashMap<Uuid, EddCase>,
}

impl EddManager {
    pub fn new() -> Self {
        Self {
            cases: HashMap::new(),
        }
    }

    /// Open a new EDD case for a user.
    pub fn open_case(&mut self, user_id: Uuid, reason: &str) -> &EddCase {
        let now = Utc::now();
        let case = EddCase {
            id: Uuid::new_v4(),
            user_id,
            trigger_reason: reason.into(),
            status: EddStatus::Open,
            reviewer: None,
            source_of_funds: None,
            source_of_wealth: None,
            notes: vec![],
            created_at: now,
            updated_at: now,
        };
        let id = case.id;
        self.cases.insert(id, case);
        // SAFETY: we just inserted the entry.
        self.cases.get(&id).expect("just inserted")
    }

    /// Transition a case to a new status.
    pub fn update_status(&mut self, case_id: Uuid, status: EddStatus) -> Result<(), String> {
        let case = self.cases.get_mut(&case_id).ok_or("Case not found")?;
        case.status = status;
        case.updated_at = Utc::now();
        Ok(())
    }

    /// Append a note to a case's audit trail.
    pub fn add_note(&mut self, case_id: Uuid, note: &str) -> Result<(), String> {
        let case = self.cases.get_mut(&case_id).ok_or("Case not found")?;
        case.notes.push(note.into());
        case.updated_at = Utc::now();
        Ok(())
    }

    /// Record source-of-funds and source-of-wealth information for a case.
    pub fn set_source_info(
        &mut self,
        case_id: Uuid,
        funds: Option<String>,
        wealth: Option<String>,
    ) -> Result<(), String> {
        let case = self.cases.get_mut(&case_id).ok_or("Case not found")?;
        case.source_of_funds = funds;
        case.source_of_wealth = wealth;
        case.updated_at = Utc::now();
        Ok(())
    }

    /// Returns all cases that are not yet approved or rejected.
    pub fn pending_cases(&self) -> Vec<&EddCase> {
        self.cases
            .values()
            .filter(|c| !matches!(c.status, EddStatus::Approved | EddStatus::Rejected))
            .collect()
    }

    /// Returns all cases associated with a specific user.
    pub fn user_cases(&self, user_id: Uuid) -> Vec<&EddCase> {
        self.cases
            .values()
            .filter(|c| c.user_id == user_id)
            .collect()
    }

    /// Returns `true` if the user has at least one open (non-terminal) EDD case.
    pub fn has_pending_edd(&self, user_id: Uuid) -> bool {
        self.cases.values().any(|c| {
            c.user_id == user_id
                && !matches!(c.status, EddStatus::Approved | EddStatus::Rejected)
        })
    }

    pub fn get(&self, case_id: Uuid) -> Option<&EddCase> {
        self.cases.get(&case_id)
    }
}

impl Default for EddManager {
    fn default() -> Self {
        Self::new()
    }
}
