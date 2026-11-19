use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComplaintCategory {
    Trading,
    Withdrawal,
    Deposit,
    KYC,
    Fees,
    Technical,
    AccountAccess,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComplaintStatus {
    Open,
    Acknowledged,
    InProgress,
    AwaitingUser,
    Resolved,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComplaintPriority {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Complaint {
    pub id: Uuid,
    pub user_id: Uuid,
    pub category: ComplaintCategory,
    pub subject: String,
    pub body: String,
    pub status: ComplaintStatus,
    pub priority: ComplaintPriority,
    pub assigned_to: Option<String>,
    pub resolution: Option<String>,
    pub created_at: DateTime<Utc>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
}

/// Complaint management system with SLA tracking.
pub struct ComplaintManager {
    complaints: Vec<Complaint>,
    /// SLA: acknowledge within N hours
    pub sla_acknowledge_hours: u32,
    /// SLA: resolve within N business days
    pub sla_resolve_days: u32,
}

impl ComplaintManager {
    pub fn new() -> Self {
        Self {
            complaints: Vec::new(),
            sla_acknowledge_hours: 24,
            sla_resolve_days: 5,
        }
    }

    /// Submit a new complaint.
    pub fn submit(
        &mut self,
        user_id: Uuid,
        category: ComplaintCategory,
        subject: String,
        body: String,
    ) -> &Complaint {
        let complaint = Complaint {
            id: Uuid::new_v4(),
            user_id,
            category,
            subject,
            body,
            status: ComplaintStatus::Open,
            priority: ComplaintPriority::Medium,
            assigned_to: None,
            resolution: None,
            created_at: Utc::now(),
            acknowledged_at: None,
            resolved_at: None,
            closed_at: None,
        };
        self.complaints.push(complaint);
        self.complaints.last().expect("just pushed")
    }

    /// Acknowledge a complaint (admin).
    pub fn acknowledge(&mut self, complaint_id: Uuid, assigned_to: &str) -> Result<(), String> {
        let c = self
            .complaints
            .iter_mut()
            .find(|c| c.id == complaint_id)
            .ok_or_else(|| "Complaint not found".to_string())?;
        c.status = ComplaintStatus::Acknowledged;
        c.acknowledged_at = Some(Utc::now());
        c.assigned_to = Some(assigned_to.to_string());
        Ok(())
    }

    /// Update complaint status.
    pub fn update_status(
        &mut self,
        complaint_id: Uuid,
        status: ComplaintStatus,
        resolution: Option<String>,
    ) -> Result<(), String> {
        let c = self
            .complaints
            .iter_mut()
            .find(|c| c.id == complaint_id)
            .ok_or_else(|| "Complaint not found".to_string())?;
        c.status = status;
        if let Some(res) = resolution {
            c.resolution = Some(res);
        }
        match status {
            ComplaintStatus::Resolved => c.resolved_at = Some(Utc::now()),
            ComplaintStatus::Closed => c.closed_at = Some(Utc::now()),
            _ => {}
        }
        Ok(())
    }

    /// Get complaints for a user.
    pub fn user_complaints(&self, user_id: Uuid) -> Vec<&Complaint> {
        self.complaints
            .iter()
            .filter(|c| c.user_id == user_id)
            .collect()
    }

    /// Get all open/pending complaints (admin).
    pub fn pending_complaints(&self) -> Vec<&Complaint> {
        self.complaints
            .iter()
            .filter(|c| {
                matches!(
                    c.status,
                    ComplaintStatus::Open
                        | ComplaintStatus::Acknowledged
                        | ComplaintStatus::InProgress
                )
            })
            .collect()
    }

    /// Get complaint by ID.
    pub fn get(&self, id: Uuid) -> Option<&Complaint> {
        self.complaints.iter().find(|c| c.id == id)
    }

    /// SLA report: complaints breaching acknowledgement or resolution SLA.
    pub fn sla_breaches(&self) -> Vec<&Complaint> {
        let now = Utc::now();
        self.complaints
            .iter()
            .filter(|c| match c.status {
                ComplaintStatus::Open => {
                    // Not acknowledged within SLA
                    let deadline = c.created_at
                        + chrono::Duration::hours(self.sla_acknowledge_hours as i64);
                    now > deadline
                }
                ComplaintStatus::Acknowledged | ComplaintStatus::InProgress => {
                    // Not resolved within SLA
                    let deadline = c.created_at
                        + chrono::Duration::days(self.sla_resolve_days as i64);
                    now > deadline
                }
                _ => false,
            })
            .collect()
    }
}

impl Default for ComplaintManager {
    fn default() -> Self {
        Self::new()
    }
}
