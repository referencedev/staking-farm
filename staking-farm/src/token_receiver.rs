use near_sdk::json_types::{U128, U64};
use near_sdk::{env, serde_json, PromiseOrValue};

use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;

use crate::*;

#[derive(Serialize, Deserialize)]
#[serde(crate = "near_sdk::serde")]
pub struct FarmingDetails {
    /// Name of the farm.
    pub name: String,
    /// Start date of the farm.
    pub start_date: U64,
    /// End date of the farm.
    pub end_date: U64,
}

#[near_bindgen]
impl FungibleTokenReceiver for StakingContract {
    /// Callback on receiving tokens by this contract.
    /// transfer reward token with specific msg indicate
    /// which farm to be deposited to.
    #[allow(unused_variables)]
    fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128> {
        assert!(
            sender_id == StakingContract::get_owner_id()
                || self.authorized_users.contains(&sender_id),
            "ERR_NOT_AUTHORIZED_USER"
        );
        let message = serde_json::from_str::<FarmingDetails>(&msg).expect("ERR_MSG_WRONG_FORMAT");
        self.internal_deposit_farm_tokens(
            &env::predecessor_account_id(),
            message.name,
            amount.0,
            message.start_date.0,
            message.end_date.0,
        );
        PromiseOrValue::Value(U128(0))
    }
}
