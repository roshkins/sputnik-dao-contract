use near_contract_standards::non_fungible_token::core::{NonFungibleTokenCore, NonFungibleTokenReceiver};
use serde::{Deserialize, Serialize};

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, UnorderedMap, UnorderedSet};
use near_sdk::json_types::{U128, U64};
use near_sdk::{
    env, ext_contract, near_bindgen, AccountId, Balance, BorshStorageKey, Duration, Gas,
    PanicOnDefault, Promise, PromiseOrValue, PromiseResult,
};

pub use user::{User, VersionedUser};

mod storage_impl;
mod user;

#[ext_contract(ext_non_fungible_token)]
trait NonFungibleTokenCore: NonFungibleTokenCore {}

#[derive(BorshStorageKey, BorshSerialize)]
enum StorageKeys {
    Users,
    ValidNFTs,
}

/// Amount of gas for fungible token transfers.
pub const GAS_FOR_NFT_TRANSFER: Gas = Gas(10_000_000_000_000);

/// Amount of gas for delegate action.
pub const GAS_FOR_DELEGATE: Gas = Gas(10_000_000_000_000);

/// Amount of gas for register action.
pub const GAS_FOR_REGISTER: Gas = Gas(10_000_000_000_000);

/// Amount of gas for undelegate action.
pub const GAS_FOR_UNDELEGATE: Gas = Gas(10_000_000_000_000);

#[ext_contract(ext_sputnik)]
pub trait Sputnik {
    fn register_delegation(&mut self, account_id: AccountId);
    fn delegate(&mut self, account_id: AccountId, amount: U128);
    fn undelegate(&mut self, account_id: AccountId, amount: U128);
}

#[near_bindgen]
#[derive(BorshSerialize, BorshDeserialize, PanicOnDefault)]
pub struct Contract {
    /// DAO owner of this staking contract.
    owner_id: AccountId,
    /// Vote token account.
    vote_token_ids: UnorderedSet<String>,
    /// Recording user deposits.
    users: LookupMap<AccountId, VersionedUser>,
    /// Total token amount deposited per token.
    total_amount: UnorderedMap<String, Balance>,
    /// Duration of unstaking. Should be over the possible voting periods.
    unstake_period: Duration,

    token_vote_weights: LookupMap<String, U128>,
}

#[ext_contract(ext_self)]
pub trait Contract {
    fn exchange_callback_post_withdraw(&mut self, sender_id: AccountId, token_id: String, amount: U128);
}

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        owner_id: AccountId,
        token_ids: UnorderedSet<String>,
        unstake_period: U64,
        token_vote_weights: LookupMap<String, U128>,
    ) -> Self {
        Self {
            owner_id: owner_id.into(),
            vote_token_ids: token_ids,
            users: LookupMap::new(StorageKeys::Users),
            total_amount: UnorderedMap::new(StorageKeys::ValidNFTs),
            unstake_period: unstake_period.0,
            token_vote_weights,
        }
    }

    /// Total number of tokens staked in this contract.
    pub fn nft_total_supply(&self) -> U128 {
        let sum = 0;
        for i in self.total_amount.iter() {
            sum += i.1;
        }
        U128(sum)
    }

    /// Sum of each token amount times it's voting weight
    pub fn total_voting_power(&self) -> U128 {
        let sum = 0;
        for i in self.total_amount.iter() {
            sum += i.1 * self.token_vote_weights.get(&i.0).unwrap_or_default();
        }
        U128(sum)
    }

    /// Total number of tokens staked by given user.
    pub fn nft_balance_of(&self, token_id: String, account_id: AccountId) -> U128 {
        let sum = 0;
        for i in self.internal_get_user(&account_id).vote_amount.iter() {
            sum += i.1;
        }
        U128(sum)
    }

    /// Returns user information.
    pub fn get_user(&self, account_id: AccountId) -> User {
        self.internal_get_user(&account_id)
    }

    /// Delegate give amount of votes to given account.
    /// If enough tokens and storage, forwards this to owner account.
    pub fn delegate(&mut self, account_id: AccountId, token_id: String, amount: U128) -> Promise {
        let sender_id = env::predecessor_account_id();
        self.internal_delegate(sender_id, account_id.clone().into(), token_id, amount.0);
        ext_sputnik::delegate(
            account_id.into(),
            amount * self.token_vote_weights.get(&token_id),
            self.owner_id.clone(),
            0,
            GAS_FOR_DELEGATE,
        )
    }

    /// Remove given amount of delegation.
    pub fn undelegate(&mut self, account_id: AccountId, token_id: String, amount: U128) -> Promise {
        let sender_id = env::predecessor_account_id();
        self.internal_undelegate(sender_id, account_id.clone().into(), token_id, amount.0);
        ext_sputnik::undelegate(
            account_id.into(),
            amount * self.token_vote_weights.get(&token_id),
            self.owner_id.clone(),
            0,
            GAS_FOR_UNDELEGATE,
        )
    }

    /// Withdraw non delegated tokens back to the user's account.
    /// If user's account is not registered, will keep funds here.
    pub fn withdraw(&mut self, token_id: String, amount: U128) -> Promise {
        let sender_id = env::predecessor_account_id();
        self.internal_withdraw(&sender_id, token_id, amount.0);

        ext_non_fungible_token::nft_transfer(sender_id.clone(), token_id.clone(), 0, None,
        token_id.clone(), 
        1,
        GAS_FOR_NFT_TRANSFER )
        .then(ext_self::exchange_callback_post_withdraw(
            sender_id,
            token_id,
            amount,
            env::current_account_id(),
            0,
            GAS_FOR_NFT_TRANSFER,
        ))
    }

    #[private]
    pub fn exchange_callback_post_withdraw(
        &mut self,
        sender_id: AccountId,
        token_id: String,
        amount: U128,
    ) {
        assert_eq!(
            env::promise_results_count(),
            1,
            "ERR_CALLBACK_POST_WITHDRAW_INVALID",
        );
        match env::promise_result(0) {
            PromiseResult::NotReady => unreachable!(),
            PromiseResult::Successful(_) => {}
            PromiseResult::Failed => {
                // This reverts the changes from withdraw function.
                self.internal_deposit(&sender_id, token_id, amount.0);
            }
        };
    }
}

#[near_bindgen]
impl NonFungibleTokenReceiver for Contract {
    fn nft_on_transfer(
        &mut self,
        sender_id: AccountId,
        previous_owner_id: AccountId,
        token_id: near_contract_standards::non_fungible_token::TokenId,
        msg: String,
    ) -> PromiseOrValue<bool> {
        assert_eq!(
            self.vote_token_ids,
            env::predecessor_account_id(),
            "ERR_INVALID_TOKEN"
        );
        assert!(msg.is_empty(), "ERR_INVALID_MESSAGE");
        //TODO: Weight vote token amount by NFT, right now 1 NFT = 1 Vote.
        self.internal_deposit(&sender_id, token_id, self.token_vote_weights.get(&token_id));
        PromiseOrValue::Value(false)
    }
}

#[cfg(test)]
mod tests {
    use near_contract_standards::non_fungible_token::TokenId;
    use near_contract_standards::storage_management::StorageManagement;
    use near_sdk::json_types::U64;
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::testing_env;

    use near_sdk_sim::to_yocto;

    use super::*;

    #[test]
    fn test_basics() {
        let period = 1000;
        const nft_id: TokenId = TokenId("TEST_NFT");
        let mut context = VMContextBuilder::new();

        testing_env!(context.predecessor_account_id(accounts(0)).build());
        let mut contract = Contract::new(accounts(0), accounts(1).to_string(), U64(period));
        testing_env!(context.attached_deposit(to_yocto("1")).build());
        contract.storage_deposit(Some(accounts(2)), None);
        testing_env!(context.predecessor_account_id(accounts(1)).build());
        contract.nft_on_transfer(accounts(2), accounts(2), nft_id, "".to_string());
        assert_eq!(contract.token_total_supply().0, 1);
        assert_eq!(contract.nft_balance_of(accounts(2)).0, 1);
        testing_env!(context.predecessor_account_id(accounts(2)).build());
        contract.withdraw(U128(1));
        assert_eq!(contract.ft_total_supply().0, to_yocto("50"));
        assert_eq!(contract.ft_balance_of(accounts(2)).0, to_yocto("50"));
        contract.delegate(accounts(3), U128(to_yocto("10")));
        let user = contract.get_user(accounts(2));
        assert_eq!(user.delegated_amount(), to_yocto("10"));
        contract.undelegate(accounts(3), U128(to_yocto("10")));
        let user = contract.get_user(accounts(2));
        assert_eq!(user.delegated_amount(), 0);
        assert_eq!(user.next_action_timestamp, U64(period));
    }
}
