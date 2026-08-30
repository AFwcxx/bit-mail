use uuid::Uuid;

use crate::{
    Result,
    repository::{AccountConfig, Repository},
};

#[derive(Debug, PartialEq, Eq)]
pub struct AccountStatus {
    pub account_id: Uuid,
    pub alias: String,
    pub pending: usize,
    pub read: usize,
    pub delete: usize,
    pub backlog_remaining: Option<bool>,
    pub last_successful_pull_ms: Option<u64>,
    pub last_successful_push_ms: Option<u64>,
}

pub fn collect(
    repository: &Repository,
    mut accounts: Vec<AccountConfig>,
) -> Result<Vec<AccountStatus>> {
    accounts.sort_by(|left, right| left.alias.cmp(&right.alias));
    accounts
        .into_iter()
        .map(|account| {
            let counts = crate::triage::work_item_counts(repository, account.id)?;
            let provider = crate::pull::provider_status(repository, account.id)?;
            Ok(AccountStatus {
                account_id: account.id,
                alias: account.alias,
                pending: counts.pending,
                read: counts.read,
                delete: counts.delete,
                backlog_remaining: provider.backlog_remaining,
                last_successful_pull_ms: provider.last_successful_pull_ms,
                last_successful_push_ms: provider.last_successful_push_ms,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{GitIgnorePolicy, NewAccount};

    #[test]
    fn status_is_offline_deterministic_and_reports_unknown_provider_state() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::initialize(directory.path(), GitIgnorePolicy::Never).unwrap();
        let second = repository
            .create_account(NewAccount {
                alias: "zeta",
                provider: "gmail",
                provider_identity: None,
                credential_profile: None,
            })
            .unwrap();
        let first = repository
            .create_account(NewAccount {
                alias: "alpha",
                provider: "gmail",
                provider_identity: None,
                credential_profile: None,
            })
            .unwrap();

        let report = collect(&repository, vec![second, first]).unwrap();
        assert_eq!(
            report
                .iter()
                .map(|item| item.alias.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert!(report.iter().all(|item| item.backlog_remaining.is_none()));
        assert!(
            report
                .iter()
                .all(|item| item.pending + item.read + item.delete == 0)
        );
    }
}
