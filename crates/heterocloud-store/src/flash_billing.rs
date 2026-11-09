use heterocloud_domain::{OrganizationId, UserId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{Store, StoreError};

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
#[serde(deny_unknown_fields)]
pub struct FlashRateCard {
    pub effective_week_start: i64,
    pub currency: String,
    pub cpu_microunits_per_vcpu_hour: i64,
    pub memory_microunits_per_gib_hour: i64,
    pub gpu_microunits_per_gpu_hour: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NewFlashRateCard {
    pub currency: String,
    pub cpu_microunits_per_vcpu_hour: i64,
    pub memory_microunits_per_gib_hour: i64,
    pub gpu_microunits_per_gpu_hour: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, sqlx::FromRow)]
#[serde(deny_unknown_fields)]
pub struct FlashWeeklyBudget {
    pub organization_id: Uuid,
    pub currency: String,
    pub amount_microunits: i64,
    pub warning_percentage: i16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NewFlashWeeklyBudget {
    pub currency: String,
    pub amount_microunits: i64,
    pub warning_percentage: i16,
}

pub fn valid_currency(currency: &str) -> bool {
    currency.len() == 3 && currency.bytes().all(|byte| byte.is_ascii_uppercase())
}

impl NewFlashRateCard {
    pub fn validate(&self) -> Result<(), StoreError> {
        if !valid_currency(&self.currency)
            || [
                self.cpu_microunits_per_vcpu_hour,
                self.memory_microunits_per_gib_hour,
                self.gpu_microunits_per_gpu_hour,
            ]
            .iter()
            .any(|value| !(0..=1_000_000_000_000).contains(value))
        {
            return Err(StoreError::RequestRejected(
                "Invalid currency or Flash rates.".into(),
            ));
        }
        Ok(())
    }
}

impl NewFlashWeeklyBudget {
    pub fn validate(&self) -> Result<(), StoreError> {
        if !valid_currency(&self.currency)
            || !(1..=9_000_000_000_000_000).contains(&self.amount_microunits)
            || !(1..=100).contains(&self.warning_percentage)
        {
            return Err(StoreError::RequestRejected(
                "Invalid currency, weekly budget or warning percentage.".into(),
            ));
        }
        Ok(())
    }
}

impl Store {
    pub async fn flash_rate_cards(&self) -> Result<Vec<FlashRateCard>, StoreError> {
        Ok(sqlx::query_as::<_, FlashRateCard>(
            "SELECT effective_week_start, currency, cpu_microunits_per_vcpu_hour, \
             memory_microunits_per_gib_hour, gpu_microunits_per_gpu_hour \
             FROM flash_rate_cards ORDER BY effective_week_start DESC LIMIT 256",
        )
        .fetch_all(self.pool())
        .await?)
    }

    pub async fn flash_rate_for_week(
        &self,
        week: i64,
    ) -> Result<Option<FlashRateCard>, StoreError> {
        Ok(sqlx::query_as::<_, FlashRateCard>(
            "SELECT effective_week_start, currency, cpu_microunits_per_vcpu_hour, \
             memory_microunits_per_gib_hour, gpu_microunits_per_gpu_hour FROM flash_rate_cards \
             WHERE effective_week_start <= $1 ORDER BY effective_week_start DESC LIMIT 1",
        )
        .bind(week)
        .fetch_optional(self.pool())
        .await?)
    }

    /// Schedule at the next UTC week. The database guards the boundary so a
    /// delayed write cannot silently change an already-started week's rates.
    pub async fn schedule_flash_rate_card(
        &self,
        input: &NewFlashRateCard,
        user: UserId,
        next_week: i64,
    ) -> Result<FlashRateCard, StoreError> {
        input.validate()?;
        sqlx::query_as::<_, FlashRateCard>(
            "INSERT INTO flash_rate_cards (effective_week_start, currency, cpu_microunits_per_vcpu_hour, \
             memory_microunits_per_gib_hour, gpu_microunits_per_gpu_hour, updated_by) \
             SELECT $1, $2, $3, $4, $5, $6 WHERE $1 > \
               (floor((extract(epoch FROM now()) - 345600) / 604800) * 604800 + 345600)::bigint \
             ON CONFLICT (effective_week_start) DO UPDATE SET currency = EXCLUDED.currency, \
             cpu_microunits_per_vcpu_hour = EXCLUDED.cpu_microunits_per_vcpu_hour, \
             memory_microunits_per_gib_hour = EXCLUDED.memory_microunits_per_gib_hour, \
             gpu_microunits_per_gpu_hour = EXCLUDED.gpu_microunits_per_gpu_hour, \
             updated_by = EXCLUDED.updated_by, updated_at = now() \
             WHERE flash_rate_cards.effective_week_start > \
               (floor((extract(epoch FROM now()) - 345600) / 604800) * 604800 + 345600)::bigint \
             RETURNING effective_week_start, currency, cpu_microunits_per_vcpu_hour, \
             memory_microunits_per_gib_hour, gpu_microunits_per_gpu_hour",
        ).bind(next_week).bind(&input.currency).bind(input.cpu_microunits_per_vcpu_hour)
            .bind(input.memory_microunits_per_gib_hour).bind(input.gpu_microunits_per_gpu_hour)
            .bind(user.0).fetch_optional(self.pool()).await?.ok_or(StoreError::Conflict)
    }

    pub async fn flash_weekly_budget(
        &self,
        org: OrganizationId,
    ) -> Result<Option<FlashWeeklyBudget>, StoreError> {
        Ok(sqlx::query_as::<_, FlashWeeklyBudget>(
            "SELECT organization_id, currency, amount_microunits, warning_percentage \
             FROM flash_weekly_budgets WHERE organization_id = $1",
        )
        .bind(org.0)
        .fetch_optional(self.pool())
        .await?)
    }

    pub async fn all_flash_weekly_budgets(&self) -> Result<Vec<FlashWeeklyBudget>, StoreError> {
        Ok(sqlx::query_as::<_, FlashWeeklyBudget>(
            "SELECT organization_id, currency, amount_microunits, warning_percentage FROM flash_weekly_budgets",
        ).fetch_all(self.pool()).await?)
    }

    pub async fn set_flash_weekly_budget(
        &self,
        org: OrganizationId,
        input: &NewFlashWeeklyBudget,
    ) -> Result<FlashWeeklyBudget, StoreError> {
        input.validate()?;
        Ok(sqlx::query_as::<_, FlashWeeklyBudget>(
            "INSERT INTO flash_weekly_budgets (organization_id, currency, amount_microunits, warning_percentage) \
             VALUES ($1, $2, $3, $4) ON CONFLICT (organization_id) DO UPDATE SET \
             currency=EXCLUDED.currency, amount_microunits=EXCLUDED.amount_microunits, \
             warning_percentage=EXCLUDED.warning_percentage, updated_at=now() \
             RETURNING organization_id, currency, amount_microunits, warning_percentage",
        ).bind(org.0).bind(&input.currency).bind(input.amount_microunits).bind(input.warning_percentage)
            .fetch_one(self.pool()).await?)
    }

    pub async fn delete_flash_weekly_budget(&self, org: OrganizationId) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM flash_weekly_budgets WHERE organization_id=$1")
            .bind(org.0)
            .execute(self.pool())
            .await?;
        Ok(())
    }
}
