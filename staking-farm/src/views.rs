use near_sdk::json_types::{U128, U64};
use near_sdk::{env, AccountId};

use crate::internal::ZERO_ADDRESS;
use crate::owner::{FACTORY_KEY, OWNER_KEY};
use crate::Farm;
use crate::*;

#[derive(Serialize, Deserialize, Clone)]
#[serde(crate = "near_sdk::serde")]
pub struct HumanReadableFarm {
    pub farm_id: u64,
    pub name: String,
    pub token_id: AccountId,
    pub amount: U128,
    pub start_date: U64,
    pub end_date: U64,
    pub active: bool,
}

impl HumanReadableFarm {
    fn from(farm_id: u64, farm: Farm) -> Self {
        let active = farm.is_active();
        HumanReadableFarm {
            farm_id,
            name: farm.name,
            token_id: farm.token_id,
            amount: U128(farm.amount),
            start_date: U64(farm.start_date),
            end_date: U64(farm.end_date),
            active,
        }
    }
}

/// Represents an account structure readable by humans.
#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct HumanReadableAccount {
    pub account_id: AccountId,
    /// The unstaked balance that can be withdrawn or staked.
    pub unstaked_balance: U128,
    /// The amount balance staked at the current "stake" share price.
    pub staked_balance: U128,
    /// Whether the unstaked balance is available for withdrawal now.
    pub can_withdraw: bool,
}

#[near_bindgen]
impl StakingContract {
    ///
    /// OWNER
    ///

    /// Returns current contract version.
    pub fn get_version() -> String {
        format!("{}:{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
    }

    /// Returns current owner from the storage.
    pub fn get_owner_id() -> AccountId {
        AccountId::new_unchecked(
            String::from_utf8(env::storage_read(OWNER_KEY).expect("MUST HAVE OWNER"))
                .expect("INTERNAL_FAIL"),
        )
    }

    /// Returns current contract factory.
    pub fn get_factory_id() -> AccountId {
        AccountId::new_unchecked(
            String::from_utf8(env::storage_read(FACTORY_KEY).expect("MUST HAVE FACTORY"))
                .expect("INTERNAL_FAIL"),
        )
    }

    /// Return all authorized users.
    pub fn get_authorized_users(&self) -> Vec<AccountId> {
        self.authorized_users.to_vec()
    }

    /// Return all authorized tokens.
    pub fn get_authorized_farm_tokens(&self) -> Vec<AccountId> {
        self.authorized_farm_tokens.to_vec()
    }

    ///
    /// FARMS
    ///

    pub fn get_active_farms(&self) -> Vec<HumanReadableFarm> {
        self.active_farms
            .iter()
            .map(|&index| HumanReadableFarm::from(index, self.farms.get(index).unwrap()))
            .collect()
    }

    pub fn get_farms(&self, from_index: u64, limit: u64) -> Vec<HumanReadableFarm> {
        (from_index..std::cmp::min(from_index + limit, self.farms.len()))
            .map(|index| HumanReadableFarm::from(index, self.farms.get(index).unwrap()))
            .collect()
    }

    pub fn get_farm(&self, farm_id: u64) -> HumanReadableFarm {
        HumanReadableFarm::from(farm_id, self.internal_get_farm(farm_id))
    }

    pub fn get_unclaimed_reward(&self, account_id: AccountId, farm_id: u64) -> U128 {
        if account_id == AccountId::new_unchecked(ZERO_ADDRESS.to_string()) {
            return U128(0);
        }
        let account = self.accounts.get(&account_id).expect("ERR_NO_ACCOUNT");
        let farm = self.farms.get(farm_id).expect("ERR_NO_FARM");
        let (_rps, reward) = self.internal_unclaimed_balance(&account, farm_id, &farm);
        let prev_reward = *account.amounts.get(&farm.token_id).unwrap_or(&0);
        U128(reward + prev_reward)
    }
}
