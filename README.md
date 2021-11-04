# Stake & earn

Staking farm contract allows for validators to distribute other tokens to the delegators.

This allows to attract more capital to the validator while ensuring more robust token distribution of the new tokens.

## Authorized users

Because of storage and computational limitations, the contract can only store a fixed number of farms.
To avoid farm spam, only authorized users can deposit tokens. 
Owner of the contract can manager authorized users or can deposit farms itself.

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

----

Corner cases:
 - limit number of active farms. remove non-active farms
 - staking from lockup for gas
 - upgrading from factory via hash
 - claiming to different predecessor with lockup (metapool)
