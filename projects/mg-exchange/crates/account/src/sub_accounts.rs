use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use thiserror::Error;
use uuid::Uuid;

use exchange_common::UserId;

use crate::account_manager::AccountManager;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SubAccountError {
    #[error("Maximum sub-account limit ({0}) reached")]
    MaxSubAccountsReached(usize),

    #[error("User {0} is not a master account")]
    NotMasterAccount(UserId),

    #[error("Sub-account {0} not found")]
    SubAccountNotFound(UserId),
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// A single sub-account belonging to a master user.
#[derive(Debug, Clone)]
pub struct SubAccount {
    pub id: UserId,
    pub label: String,
    pub created_at: DateTime<Utc>,
    pub is_active: bool,
}

/// All sub-accounts that belong to one master user.
#[derive(Debug, Clone)]
pub struct SubAccountGroup {
    pub master_id: UserId,
    pub sub_accounts: Vec<SubAccount>,
    /// Hard limit on the number of sub-accounts. Defaults to 20.
    pub max_subs: usize,
    pub created_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Aggregated portfolio view
// ---------------------------------------------------------------------------

/// Per-account balance snapshot for portfolio aggregation.
#[derive(Debug, Clone)]
pub struct AccountSummary {
    pub user_id: UserId,
    /// Human-readable label. For the master account this is "master".
    pub label: String,
    /// asset → total balance (locked + available).
    pub balances: HashMap<String, Decimal>,
}

/// Unified portfolio view across master + all sub-accounts.
#[derive(Debug, Clone)]
pub struct PortfolioSummary {
    pub master_id: UserId,
    /// One entry per account (master first, then subs in creation order).
    pub accounts: Vec<AccountSummary>,
    /// Per-asset totals across all accounts.
    pub totals: HashMap<String, Decimal>,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// Manages sub-account groups and their reverse-lookup index.
///
/// This is intentionally a pure in-memory data structure that holds no
/// reference to `AccountManager`. Balance operations stay in `AccountManager`;
/// sub-account topology lives here. The two are composed by callers.
pub struct SubAccountManager {
    /// master_id → group
    groups: HashMap<UserId, SubAccountGroup>,
    /// sub_id → master_id (reverse lookup for O(1) parent resolution)
    sub_to_master: HashMap<UserId, UserId>,
}

impl SubAccountManager {
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
            sub_to_master: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Mutations
    // -----------------------------------------------------------------------

    /// Create a new sub-account under `master_id`.
    ///
    /// Returns an error if `master_id` has already reached its sub-account
    /// limit. A group is created on the first call for a given master.
    pub fn create_sub(
        &mut self,
        master_id: UserId,
        label: impl Into<String>,
    ) -> Result<SubAccount, SubAccountError> {
        let group = self.groups.entry(master_id).or_insert_with(|| SubAccountGroup {
            master_id,
            sub_accounts: Vec::new(),
            max_subs: 20,
            created_at: Utc::now(),
        });

        if group.sub_accounts.len() >= group.max_subs {
            return Err(SubAccountError::MaxSubAccountsReached(group.max_subs));
        }

        let sub = SubAccount {
            id: Uuid::new_v4(),
            label: label.into(),
            created_at: Utc::now(),
            is_active: true,
        };

        self.sub_to_master.insert(sub.id, master_id);
        group.sub_accounts.push(sub.clone());

        tracing::info!(
            master_id = %master_id,
            sub_id = %sub.id,
            label = %sub.label,
            "Sub-account created"
        );

        Ok(sub)
    }

    /// Deactivate a sub-account. Does not delete historical data.
    ///
    /// Returns an error if `master_id` has no group or the sub is not found.
    pub fn deactivate_sub(
        &mut self,
        master_id: UserId,
        sub_id: UserId,
    ) -> Result<(), SubAccountError> {
        let group = self
            .groups
            .get_mut(&master_id)
            .ok_or(SubAccountError::NotMasterAccount(master_id))?;

        let sub = group
            .sub_accounts
            .iter_mut()
            .find(|s| s.id == sub_id)
            .ok_or(SubAccountError::SubAccountNotFound(sub_id))?;

        sub.is_active = false;

        tracing::info!(
            master_id = %master_id,
            sub_id = %sub_id,
            "Sub-account deactivated"
        );

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Return all sub-accounts (active and inactive) for a master.
    pub fn list_subs(&self, master_id: UserId) -> Vec<&SubAccount> {
        self.groups
            .get(&master_id)
            .map(|g| g.sub_accounts.iter().collect())
            .unwrap_or_default()
    }

    /// Return true if the user has ever created at least one sub-account
    /// (i.e., a group entry exists for them).
    pub fn is_master(&self, user_id: UserId) -> bool {
        self.groups.contains_key(&user_id)
    }

    /// Return true if this user ID was created as a sub-account.
    pub fn is_sub_account(&self, user_id: UserId) -> bool {
        self.sub_to_master.contains_key(&user_id)
    }

    /// Resolve the master for a sub-account. Returns `None` for master/unknown users.
    pub fn get_master(&self, sub_id: UserId) -> Option<UserId> {
        self.sub_to_master.get(&sub_id).copied()
    }

    // -----------------------------------------------------------------------
    // Portfolio aggregation
    // -----------------------------------------------------------------------

    /// Aggregate balances across the master account and all its sub-accounts.
    ///
    /// Accounts that have no balance entry in `AccountManager` are included
    /// with an empty balance map. This is correct: a freshly-created sub-account
    /// has no deposits yet.
    pub fn portfolio_summary(
        &self,
        master_id: UserId,
        accounts: &AccountManager,
    ) -> PortfolioSummary {
        let mut result_accounts: Vec<AccountSummary> = Vec::new();
        let mut totals: HashMap<String, Decimal> = HashMap::new();

        // Master account first
        let master_balances = collect_balances(master_id, accounts);
        accumulate(&master_balances, &mut totals);
        result_accounts.push(AccountSummary {
            user_id: master_id,
            label: "master".to_string(),
            balances: master_balances,
        });

        // Sub-accounts in creation order
        if let Some(group) = self.groups.get(&master_id) {
            for sub in &group.sub_accounts {
                let sub_balances = collect_balances(sub.id, accounts);
                accumulate(&sub_balances, &mut totals);
                result_accounts.push(AccountSummary {
                    user_id: sub.id,
                    label: sub.label.clone(),
                    balances: sub_balances,
                });
            }
        }

        PortfolioSummary {
            master_id,
            accounts: result_accounts,
            totals,
        }
    }
}

impl Default for SubAccountManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Collect the total balance (locked + available) per asset for one user.
fn collect_balances(user_id: UserId, accounts: &AccountManager) -> HashMap<String, Decimal> {
    accounts
        .get_all_balances(&user_id)
        .map(|map| map.iter().map(|(asset, bal)| (asset.clone(), bal.total())).collect())
        .unwrap_or_default()
}

/// Add `src` into `dst`, summing values for shared assets.
fn accumulate(src: &HashMap<String, Decimal>, dst: &mut HashMap<String, Decimal>) {
    for (asset, amount) in src {
        *dst.entry(asset.clone()).or_insert(Decimal::ZERO) += amount;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn new_user() -> UserId {
        Uuid::new_v4()
    }

    // -----------------------------------------------------------------------
    // create_sub
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_sub_returns_sub_account() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        let sub = mgr.create_sub(master, "trading").unwrap();
        assert_eq!(sub.label, "trading");
        assert!(sub.is_active);
    }

    #[test]
    fn test_create_sub_auto_creates_group() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        assert!(!mgr.is_master(master));
        mgr.create_sub(master, "sub1").unwrap();
        assert!(mgr.is_master(master));
    }

    #[test]
    fn test_create_multiple_subs() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        let s1 = mgr.create_sub(master, "s1").unwrap();
        let s2 = mgr.create_sub(master, "s2").unwrap();
        assert_ne!(s1.id, s2.id);
        assert_eq!(mgr.list_subs(master).len(), 2);
    }

    #[test]
    fn test_create_sub_max_limit_default_20() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        for i in 0..20 {
            mgr.create_sub(master, format!("sub{i}")).unwrap();
        }
        let err = mgr.create_sub(master, "overflow").unwrap_err();
        assert_eq!(err, SubAccountError::MaxSubAccountsReached(20));
    }

    #[test]
    fn test_create_sub_custom_max() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        // Create group with custom limit
        mgr.groups.insert(master, SubAccountGroup {
            master_id: master,
            sub_accounts: Vec::new(),
            max_subs: 3,
            created_at: Utc::now(),
        });
        for i in 0..3 {
            mgr.create_sub(master, format!("s{i}")).unwrap();
        }
        let err = mgr.create_sub(master, "overflow").unwrap_err();
        assert_eq!(err, SubAccountError::MaxSubAccountsReached(3));
    }

    // -----------------------------------------------------------------------
    // list_subs
    // -----------------------------------------------------------------------

    #[test]
    fn test_list_subs_empty_for_unknown_master() {
        let mgr = SubAccountManager::new();
        assert!(mgr.list_subs(new_user()).is_empty());
    }

    #[test]
    fn test_list_subs_includes_inactive() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        let sub = mgr.create_sub(master, "s1").unwrap();
        mgr.deactivate_sub(master, sub.id).unwrap();
        assert_eq!(mgr.list_subs(master).len(), 1);
        assert!(!mgr.list_subs(master)[0].is_active);
    }

    // -----------------------------------------------------------------------
    // is_master / is_sub_account
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_master_true_after_creating_sub() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        assert!(!mgr.is_master(master));
        mgr.create_sub(master, "x").unwrap();
        assert!(mgr.is_master(master));
    }

    #[test]
    fn test_is_sub_account_true_for_created_sub() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        let sub = mgr.create_sub(master, "x").unwrap();
        assert!(mgr.is_sub_account(sub.id));
        assert!(!mgr.is_sub_account(master));
    }

    #[test]
    fn test_is_sub_account_false_for_regular_user() {
        let mgr = SubAccountManager::new();
        assert!(!mgr.is_sub_account(new_user()));
    }

    // -----------------------------------------------------------------------
    // get_master (reverse lookup)
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_master_returns_correct_parent() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        let sub = mgr.create_sub(master, "sub").unwrap();
        assert_eq!(mgr.get_master(sub.id), Some(master));
    }

    #[test]
    fn test_get_master_returns_none_for_non_sub() {
        let mgr = SubAccountManager::new();
        assert!(mgr.get_master(new_user()).is_none());
    }

    #[test]
    fn test_get_master_different_masters_independent() {
        let mut mgr = SubAccountManager::new();
        let m1 = new_user();
        let m2 = new_user();
        let s1 = mgr.create_sub(m1, "a").unwrap();
        let s2 = mgr.create_sub(m2, "b").unwrap();
        assert_eq!(mgr.get_master(s1.id), Some(m1));
        assert_eq!(mgr.get_master(s2.id), Some(m2));
    }

    // -----------------------------------------------------------------------
    // deactivate_sub
    // -----------------------------------------------------------------------

    #[test]
    fn test_deactivate_sub_sets_inactive() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        let sub = mgr.create_sub(master, "s").unwrap();
        mgr.deactivate_sub(master, sub.id).unwrap();
        let subs = mgr.list_subs(master);
        assert!(!subs[0].is_active);
    }

    #[test]
    fn test_deactivate_sub_not_found_errors() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        mgr.create_sub(master, "s").unwrap();
        let err = mgr.deactivate_sub(master, new_user()).unwrap_err();
        assert!(matches!(err, SubAccountError::SubAccountNotFound(_)));
    }

    #[test]
    fn test_deactivate_sub_unknown_master_errors() {
        let mut mgr = SubAccountManager::new();
        let master = new_user();
        let err = mgr.deactivate_sub(master, new_user()).unwrap_err();
        assert!(matches!(err, SubAccountError::NotMasterAccount(_)));
    }

    // -----------------------------------------------------------------------
    // portfolio_summary
    // -----------------------------------------------------------------------

    #[test]
    fn test_portfolio_summary_master_only() {
        let sub_mgr = SubAccountManager::new();
        let mut acc_mgr = AccountManager::new();
        let master = new_user();
        acc_mgr.create_account(master);
        acc_mgr.deposit(master, "USDT", dec!(1000)).unwrap();

        let summary = sub_mgr.portfolio_summary(master, &acc_mgr);
        assert_eq!(summary.master_id, master);
        assert_eq!(summary.accounts.len(), 1);
        assert_eq!(summary.totals["USDT"], dec!(1000));
    }

    #[test]
    fn test_portfolio_summary_aggregates_subs() {
        let mut sub_mgr = SubAccountManager::new();
        let mut acc_mgr = AccountManager::new();

        let master = new_user();
        let sub1 = sub_mgr.create_sub(master, "sub1").unwrap();
        let sub2 = sub_mgr.create_sub(master, "sub2").unwrap();

        // Create accounts and add balances
        acc_mgr.create_account(master);
        acc_mgr.create_account(sub1.id);
        acc_mgr.create_account(sub2.id);

        acc_mgr.deposit(master, "USDT", dec!(1000)).unwrap();
        acc_mgr.deposit(sub1.id, "USDT", dec!(500)).unwrap();
        acc_mgr.deposit(sub2.id, "BTC", dec!(1)).unwrap();

        let summary = sub_mgr.portfolio_summary(master, &acc_mgr);
        assert_eq!(summary.accounts.len(), 3);
        assert_eq!(summary.totals["USDT"], dec!(1500));
        assert_eq!(summary.totals["BTC"], dec!(1));
    }

    #[test]
    fn test_portfolio_summary_master_label() {
        let sub_mgr = SubAccountManager::new();
        let mut acc_mgr = AccountManager::new();
        let master = new_user();
        acc_mgr.create_account(master);

        let summary = sub_mgr.portfolio_summary(master, &acc_mgr);
        assert_eq!(summary.accounts[0].label, "master");
    }

    #[test]
    fn test_portfolio_summary_accounts_order() {
        let mut sub_mgr = SubAccountManager::new();
        let mut acc_mgr = AccountManager::new();
        let master = new_user();
        let s1 = sub_mgr.create_sub(master, "first").unwrap();
        let s2 = sub_mgr.create_sub(master, "second").unwrap();

        acc_mgr.create_account(master);
        acc_mgr.create_account(s1.id);
        acc_mgr.create_account(s2.id);

        let summary = sub_mgr.portfolio_summary(master, &acc_mgr);
        // master is always index 0
        assert_eq!(summary.accounts[0].user_id, master);
        assert_eq!(summary.accounts[1].label, "first");
        assert_eq!(summary.accounts[2].label, "second");
    }

    #[test]
    fn test_portfolio_summary_empty_sub_has_empty_balances() {
        let mut sub_mgr = SubAccountManager::new();
        let mut acc_mgr = AccountManager::new();
        let master = new_user();
        let sub = sub_mgr.create_sub(master, "empty").unwrap();
        acc_mgr.create_account(master);
        // sub account exists in sub_mgr but NOT in acc_mgr (no deposit yet)

        let summary = sub_mgr.portfolio_summary(master, &acc_mgr);
        let sub_summary = summary.accounts.iter().find(|a| a.user_id == sub.id).unwrap();
        assert!(sub_summary.balances.is_empty());
    }
}
