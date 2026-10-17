use super::ProjectStoreWriter;
use super::ProjectWriteError;
use crate::PreparedDomainUpgrade;
use sqlx::Connection;

impl ProjectStoreWriter {
    /// Explicitly upgrade this predecessor owner under one immediate reservation.
    /// All refusals, failures and cancellation leave it close-only. Retain the
    /// plan, await close, and reconcile a fresh snapshot if commit is uncertain.
    /// Only acknowledged commit synchronously switches this owner's manifest.
    /// New predecessor readers refuse afterward, but held WAL snapshots can
    /// finish and escaped raw read connections cannot be revoked here. This is
    /// not old-client exclusion or native conversion/activation.
    pub async fn upgrade_domains(
        &mut self,
        plan: &PreparedDomainUpgrade,
    ) -> Result<(), ProjectWriteError> {
        if !self.ready {
            return Err(ProjectWriteError::UnfinishedOperation);
        }
        self.ready = false;
        let mut transaction = self.connection.begin_with("BEGIN IMMEDIATE").await?;
        let result = async {
            if self.domains.validate(&mut transaction).await? != self.identity {
                return Err(ProjectWriteError::IdentityMismatch);
            }
            plan.apply(&mut transaction, &self.identity).await
        }.await;
        let successor = match result {
            Ok(successor) => successor,
            Err(error) => {
                transaction.rollback().await?;
                return Err(error);
            }
        };
        transaction.commit().await?;
        self.domains = successor;
        self.ready = true;
        Ok(())
    }
}

