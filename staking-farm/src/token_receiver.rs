use near_sdk::json_types::{U128, U64};
use near_sdk::{env, serde_json, PromiseOrValue};

use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;

use crate::*;

const ERR_MSG_REQUIRED_FIELD: &str = "ERR_MSG_REQUIRED_FIELD";

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct FarmingDetails {
    /// Name of the farm.
    pub name: Option<String>,
    /// Start date of the farm.
    /// If the farm ID is given, the new start date can only be provided if the farm hasn't started.
    pub start_date: Option<U64>,
    /// End date of the farm.
    pub end_date: U64,
    /// Existing farm ID.
    pub farm_id: Option<u64>,
}

#[near_bindgen]
impl FungibleTokenReceiver for StakingContract {
    /// Callback on receiving tokens by this contract.
    /// transfer reward token with specific msg indicate
    /// which farm to be deposited to.
    fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        assert!(
            self.authorized_farm_tokens
                .contains(&env::predecessor_account_id()),
            "ERR_NOT_AUTHORIZED_TOKEN"
        );
        assert!(
            sender_id == StakingContract::internal_get_owner_id()
                || self.authorized_users.contains(&sender_id),
            "ERR_NOT_AUTHORIZED_USER"
        );
        let message = serde_json::from_str::<FarmingDetails>(&msg).expect("ERR_MSG_WRONG_FORMAT");
        if let Some(farm_id) = message.farm_id {
            self.internal_add_farm_tokens(
                &env::predecessor_account_id(),
                farm_id,
                amount.0,
                message.start_date.map(|start_date| start_date.0),
                message.end_date.0,
            );
        } else {
            assert!(
                self.active_farms.len() <= MAX_NUM_ACTIVE_FARMS,
                "ERR_TOO_MANY_ACTIVE_FARMS"
            );
            self.internal_deposit_farm_tokens(
                &env::predecessor_account_id(),
                message.name.expect(ERR_MSG_REQUIRED_FIELD),
                amount.0,
                message.start_date.expect(ERR_MSG_REQUIRED_FIELD).0,
                message.end_date.0,
            );
        }
        PromiseOrValue::Value(U128(0))
    }
}
