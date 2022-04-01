# Stake & earn

Staking farm contract allows for validators to distribute other tokens to the delegators.

This allows to attract more capital to the validator while ensuring more robust token distribution of the new tokens.

## Authorized users

Because of storage and computational limitations, the contract can only store a fixed number of farms.
To avoid farm spam, only authorized users can deposit tokens. 
Owner of the contract can manage authorized users or can deposit farms itself.

## Create new farm

Use `ft_transfer_call` of the token to the staking farm by an authorized user to create new farm.

Farm contains next fields:
 - name: String,
 - token_id: AccountId,
 - amount: Balance,
 - start_date: Timestamp,
 - end_date: Timestamp,
 - last_distribution: RewardDistribution,

## Upgradability

Staking Farm contract supports upgradability from the specific factory contract.
This is done to ensure that both contract owner and general community agree on the contract upgrade before it happens.

The procedure for upgrades is as follows:
 - Staking contract has the `factory_id` specified. This `factory_id` should be governed by the users or Foundation that users trust. 
 - Factory contract contains whitelisted set of contracts, addressed by hash.
 - Contract owner calls `upgrade(contract_hash)` method, which loads contract bytecode from `factory_id` and upgrade itself in-place.

To avoid potential issues with state serialization failures due to upgrades, the owner information is stored outside of the STATE storage.
This ensures that if new contracts has similar `upgrade` method that doesn't use state, even if contract got into wrong state after upgrade it is resolvable.

## Burning rewards

The staking reward contract has a feature to burn part of the rewards.
NEAR doesn't have currently a integrated burning logic, so instead a `ZERO_ADDRESS` is used. This is an implicit address of `0` and that doesn't have any access keys: https://explorer.mainnet.near.org/accounts/0000000000000000000000000000000000000000000000000000000000000000

The burning itself is done in a 3 steps:
 - When epoch ends and `ping` is called, the amount of rewawrds allocated to burn will be transferred to `ZERO_ADDRESS` address via shares. This shares are still staked.
 - Anyone can call `unstake_burn`, which will unstake all the currently staked shares on `ZERO_ADDRESS`.
 - After 36 hours of unstaking, anyone can call `burn` to actually transfer funds to `ZERO_ADDRESS`.

This is done because transferring immediately rewards to `ZERO_ADDRESS` is impossible as they are already staked when allocated.
Anyone can call `unstake_burn` and `burn`, similarly how anyone can call `ping` on the staking pool to kick the calculations.

TODO: the imporvement to this method, would be to unstake that amount direclty on `ping` and just let it be burnt via the subsequent `burn` call.
