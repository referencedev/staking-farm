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


# Staking / Delegation contract

This contract provides a way for other users to delegate funds to a single validation node.

Implements the https://github.com/nearprotocol/NEPs/pull/27 standard.

There are three different roles:
- The staking pool contract account `my_validator`. A key-less account with the contract that pools funds.
- The owner of the staking contract `owner`. Owner runs the validator node on behalf of the staking pool account.
- Delegator accounts `user1`, `user2`, etc. Accounts that want to stake their funds with the pool.

The owner can setup such contract and validate on behalf of this contract in their node.
Any other user can send their tokens to the contract, which will be pooled together and increase the total stake.
These users accrue rewards (subtracted fees set by the owner).
Then they can unstake and withdraw their balance after some unlocking period.

## Staking pool implementation details

For secure operation of the staking pool, the contract should not have any access keys.
Otherwise the contract account may issue a transaction that can violate the contract guarantees.

After users deposit tokens to the contract, they can stake some or all of them to receive "stake" shares.
The staking contract uses two internal pools for managing the stake and stake rewards. In the first pool the rewards are being restaked again. In the second pool they are not being restaked, but saved so every user who indicates that he wants his rewards sent to different address, can withdraw them at any time.
The price of a "stake" share can be defined as the total amount of staked tokens divided by the the total amount of "stake" shares.
The number of "stake" shares is always less than the number of the staked tokens, so the price of single "stake" share is not less than `1`.

### Initialization

A contract has to be initialized with the following parameters:
- `owner_id` - `string` the account ID of the contract owner. This account will be able to call owner-only methods. E.g. `owner`
- `stake_public_key` - `string` the initial public key that will be used for staking on behalf of the contract's account in base58 ED25519 curve. E.g. `KuTCtARNzxZQ3YvXDeLjx83FDqxv2SdQTSbiq876zR7`
- `reward_fee_fraction` - `json serialized object` the initial value of the fraction of the reward that the owner charges delegators for running the node.
The fraction is defined by the numerator and denumerator with `u32` types. E.g. `{numerator: 10, denominator: 100}` defines `10%` reward fee.
- `burn_fee_fraction` - `json serialized object` the staking reward contract has a feature to burn part of the rewards.
On every ping part of the staking rewards are being burnt saved to the zero account.

During the initialization the contract checks validity of the input and initializes the contract.
The contract shouldn't have locked balance during the initialization.

At the initialization the contract allocates one trillion yocto NEAR tokens towards "stake" share price guarantees.
This fund is later used to adjust the the amount of staked and unstaked tokens due to rounding error.
For each stake and unstake action, the contract may spend at most 1 yocto NEAR from this fund (implicitly).

The current total balance (except for the "stake" share price guarantee amount) is converted to shares and will be staked (after the next action).
This balance can never be unstaked or withdrawn from the contract.
It's used to maintain the minimum number of shares, as well as help pay for the potentially growing contract storage.

### Delegator accounts

The contract maintains account information per delegator associated with the hash of the delegator's account ID.

The information contains:
- Unstaked balance of the account.
- Number of "stake" shares.
- The minimum epoch height when the unstaked balance can be withdrawn. Initially zero.
- Farmed tokens amount.
- Not staked staking rewards (only for accounts that want their staking rewards to be sent to another address).

A delegator can do the following actions:

#### Deposit

When a delegator account first deposits funds to the contract, the internal account is created and credited with the
attached amount of unstaked tokens. The delegator account can decide whether he wants his staking rewards to be restaked again on every epoch.

#### Stake

When an account wants to stake a given amount, the contract distributes any farming rewards, then the contract calculates the number of "stake" shares (`num_shares`) and the actual rounded stake amount (`amount`).
The unstaked balance of the account is decreased by `amount`, the number of "stake" shares of the account is increased by `num_shares`.
The contract increases the total number of staked tokens and the total number of "stake" shares. Then the contract restakes.

#### Unstake

When an account wants to unstake a given amount, the contract distributes any farming rewards, then the contract calculates the number of "stake" shares needed (`num_shares`) and
the actual required rounded unstake amount (`amount`). It's calculated based on the current total price of "stake" shares.
The unstaked balance of the account is increased by `amount`, the number of "stake" shares of the account is decreased by `num_shares`.
The minimum epoch height when the account can withdraw is set to the current epoch height increased by `4`.
The contract decreases the total number of staked tokens and the total number of "stake" shares. Then the contract restakes.

#### Withdraw

When an account wants to withdraw, the contract checks the minimum epoch height of this account and checks the amount.
Then sends the transfer and decreases the unstaked balance of the account.

#### Ping

Calls the internal function to distribute rewards if the blockchain epoch switched. The contract will restake part of the rewards, the other part will be available for withdraw, for those accounts that indicated using this feature.
### Reward distribution

Before every action the contract calls method `internal_ping`.
This method distributes rewards towards active delegators when the blockchain epoch switches.
The rewards might be given due to staking and also because the contract earns gas fee rebates for every function call.
Note, the if someone accidentally (or intentionally) transfers tokens to the contract (without function call), then
tokens from the transfer will be distributed to the active stake participants of the contract in the next epoch.
Note, in a rare scenario, where the owner withdraws tokens and while the call is being processed deletes their account, the
withdraw transfer will fail and the tokens will be returned to the staking pool. These tokens will also be distributed as
a reward in the next epoch.

The method first checks that the current epoch is different from the last epoch, and if it's not changed exits the method.

The reward are computed the following way. The contract keeps track of the last known total account balance.
This balance consist of the initial contract balance, and all delegator account balances (including the owner) and all accumulated rewards.
(Validation rewards are added automatically at the beginning of the epoch, while contract execution gas rebates are added after each transaction)

When the method is called the contract uses the current total account balance (without attached deposit) and the subtracts the last total account balance.
The difference is the total reward that has to be distributed.

The fraction of the reward is awarded to the contract owner. The fraction is configurable by the owner, but can't exceed 100%.
Note, that it might be unfair for the participants of the pool if the owner changes reward fee. But this owner will lose trust of the
participants and it will lose future revenue in the long term. This should be enough to prevent owner from abusing reward fee.
It could also be the case that they could change the reward fee to make their pool more attractive.

The remaining part of the reward is distributed between the two inner pools. The calculation is made by combining the total staked balance of each pool and then distributed using the formula (total staked balance pool A) / (total staked balance pool A + total staked balance pool B). After figuring out what rewards goes to each pool, relative to the pool that staked its reward, the reward is added to the total staked balance of the inner pool, in the case of the other inner pool, that doesnt restake its rewards, the amount of reward is distributed between the accounts in this pool. Using the "Scalable Reward Distribution with Changing Stake Sizes" algorithm (https://solmaz.io/2019/02/24/scalable-reward-changing). For the pool that restakes rewards this action increases the price of each "stake" share without
changing the amount of "stake" shares owned by different accounts. Which is effectively distributing the reward based on the number of shares.
For the pool that doesnt restake rewards, the total staked balance remains the same, but the rewards for each account are increased.

The owner's reward is converted into "stake" shares at the new price and added to the owner's account.
It's done similarly to `stake` method but without debiting the unstaked balance of owner's account.

Once the rewards are distributed the contract remembers the new total balance.

### NB! If you want to use this staking pool contract, use nbh-sp-factory.testnet account.

## Owner-only methods

Contract owner can do the following:
- Change owner.
- Change public staking key. This action restakes with the new key.
- Change reward fee fraction.
- Vote on behalf of the pool. This is needed for the NEAR chain governance, and can be discussed in the following NEP: https://github.com/nearprotocol/NEPs/pull/62
- Pause and resume staking. When paused, the pool account unstakes everything (stakes 0) and doesn't restake.
It doesn't affect the staking shares or reward distribution. Pausing is useful for node maintenance. Note, the contract is not paused by default.
- Add/Remove authorized users to be able to send farming tokens to the contract

## Staking pool contract guarantees and invariants

This staking pool implementation guarantees the required properties of the staking pool standard:

- The contract can't lose or lock tokens of users.
- If a user deposited X, the user should be able to withdraw at least X.
- If a user successfully staked X, the user can unstake at least X.
- The contract should not lock unstaked funds for longer than 4 epochs after unstake action.

It also has inner invariants:

- The staking pool contract is secure if it doesn't have any access keys.
- The price of a "stake" is always at least `1`.
- The price of a "stake" share never decreases.
- The reward fee is a fraction be from `0` to `1` inclusive.
- The owner can't withdraw funds from other delegators.
- The owner can't delete the staking pool account.

NOTE: Guarantees are based on the no-slashing condition. Once slashing is introduced, the contract will no longer
provide some guarantees. Read more about slashing in [Nightshade paper](https://near.ai/nightshade).

## Changelog
### `1.1.0`
- Internal refactoring. Added functionality for not restaking rewards, but saving them so account can withdraw them at any time.
- Added new delegator methods:
    - `deposit_rewards_not_stake` - to deposit amount to the inner pool that doesnt restakes its rewards
    - `deposit_and_stake_rewards_not_stake` - to deposit and stake the attached balance in one call in the inner pool that doesnt restakes it rewards
    - `withdraw_rewards` - to withdraw all rewards, this method accepts a parameter receiver_account_id which is used for sending the rewards to
    - `get_account_not_staked_rewards` - returns amount of rewards. If the account has deposited and staked to the pool that doesnt restakes rewards, then it will receive something. Otherwise will receive 0.
- Changed existing methods:
    - `withdraw_all` - withdraws unstaked balance and also not staked rewards (if any)

### `1.0.0`
- Added farming feature, for incentivizing using other projects.