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
 