//! Supply snapshot engine for MAR holder analysis.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use solana_sdk::pubkey::Pubkey;

/// In-memory snapshot of token holder balances derived from token account updates.
#[derive(Debug)]
pub struct SupplySnapshotEngine {
    token_account_owner: HashMap<Pubkey, Pubkey>,
    token_account_amount: HashMap<Pubkey, u64>,
    owner_balances: HashMap<Pubkey, u64>,
    top_holders: TopHolders,
    observed_supply: u64,
    total_supply: Option<u64>,
}

impl SupplySnapshotEngine {
    /// Create a new snapshot engine with the provided top holders limit.
    pub fn new(top_holders_limit: usize) -> Self {
        Self {
            token_account_owner: HashMap::new(),
            token_account_amount: HashMap::new(),
            owner_balances: HashMap::new(),
            top_holders: TopHolders::new(top_holders_limit),
            observed_supply: 0,
            total_supply: None,
        }
    }

    /// Apply an update for a token account.
    pub fn apply_token_account_update(
        &mut self,
        _slot: u64,
        token_account_pubkey: Pubkey,
        owner_pubkey: Pubkey,
        _mint_pubkey: Pubkey,
        new_amount: u64,
    ) {
        let previous_amount = self
            .token_account_amount
            .get(&token_account_pubkey)
            .copied()
            .unwrap_or(0);
        let previous_owner = self.token_account_owner.get(&token_account_pubkey).copied();

        if new_amount >= previous_amount {
            self.observed_supply = self
                .observed_supply
                .saturating_add(new_amount - previous_amount);
        } else {
            self.observed_supply = self
                .observed_supply
                .saturating_sub(previous_amount - new_amount);
        }

        match previous_owner {
            Some(previous_owner) if previous_owner == owner_pubkey => {
                self.adjust_owner_balance(previous_owner, previous_amount, new_amount);
            }
            Some(previous_owner) => {
                self.adjust_owner_balance(previous_owner, previous_amount, 0);
                self.adjust_owner_balance(owner_pubkey, 0, new_amount);
            }
            None => {
                self.adjust_owner_balance(owner_pubkey, 0, new_amount);
            }
        }

        if new_amount == 0 {
            self.token_account_amount.remove(&token_account_pubkey);
            self.token_account_owner.remove(&token_account_pubkey);
        } else {
            self.token_account_amount
                .insert(token_account_pubkey, new_amount);
            self.token_account_owner
                .insert(token_account_pubkey, owner_pubkey);
        }
    }

    /// Returns top holders in descending balance order.
    pub fn get_top_holders_desc(&self) -> Vec<(Pubkey, u64)> {
        self.top_holders.top_holders_desc()
    }

    /// Observed supply from tracked token accounts.
    pub fn observed_supply(&self) -> u64 {
        self.observed_supply
    }

    /// Optional total supply value.
    pub fn total_supply(&self) -> Option<u64> {
        self.total_supply
    }

    /// Set total supply value.
    pub fn set_total_supply(&mut self, total_supply: u64) {
        self.total_supply = Some(total_supply);
    }

    fn adjust_owner_balance(&mut self, owner: Pubkey, old_amount: u64, new_amount: u64) {
        let current_balance = self.owner_balances.get(&owner).copied().unwrap_or(0);
        let updated_balance = current_balance
            .saturating_sub(old_amount)
            .saturating_add(new_amount);

        self.top_holders
            .update(owner, current_balance, updated_balance);

        if updated_balance == 0 {
            self.owner_balances.remove(&owner);
        } else {
            self.owner_balances.insert(owner, updated_balance);
        }
    }
}

impl Default for SupplySnapshotEngine {
    fn default() -> Self {
        Self::new(200)
    }
}

#[derive(Debug)]
struct TopHolders {
    limit: usize,
    balances: BTreeMap<u64, BTreeSet<Pubkey>>,
}

impl TopHolders {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            balances: BTreeMap::new(),
        }
    }

    fn update(&mut self, owner: Pubkey, old_balance: u64, new_balance: u64) {
        if old_balance == new_balance {
            return;
        }

        if old_balance > 0 {
            if let Some(owners) = self.balances.get_mut(&old_balance) {
                owners.remove(&owner);
                if owners.is_empty() {
                    self.balances.remove(&old_balance);
                }
            }
        }

        if new_balance > 0 {
            self.balances.entry(new_balance).or_default().insert(owner);
        }
    }

    fn top_holders_desc(&self) -> Vec<(Pubkey, u64)> {
        if self.limit == 0 {
            return Vec::new();
        }

        let mut holders = Vec::new();
        for (balance, owners) in self.balances.iter().rev() {
            for owner in owners {
                holders.push((*owner, *balance));
                if holders.len() >= self.limit {
                    return holders;
                }
            }
        }
        holders
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_balances_and_supply() {
        let mut engine = SupplySnapshotEngine::new(10);
        let owner_a = Pubkey::new_unique();
        let owner_b = Pubkey::new_unique();
        let account_a1 = Pubkey::new_unique();
        let account_a2 = Pubkey::new_unique();
        let account_b1 = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        engine.apply_token_account_update(1, account_a1, owner_a, mint, 100);
        engine.apply_token_account_update(1, account_a2, owner_a, mint, 25);
        engine.apply_token_account_update(1, account_b1, owner_b, mint, 50);

        assert_eq!(engine.observed_supply(), 175);
        let holders = engine.get_top_holders_desc();
        assert_eq!(holders.len(), 2);
        assert_eq!(holders[0], (owner_a, 125));
        assert_eq!(holders[1], (owner_b, 50));
    }

    #[test]
    fn respects_top_holder_limit() {
        let mut engine = SupplySnapshotEngine::new(2);
        let owner_a = Pubkey::new_unique();
        let owner_b = Pubkey::new_unique();
        let owner_c = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        engine.apply_token_account_update(1, Pubkey::new_unique(), owner_a, mint, 100);
        engine.apply_token_account_update(1, Pubkey::new_unique(), owner_b, mint, 90);
        engine.apply_token_account_update(1, Pubkey::new_unique(), owner_c, mint, 80);

        let holders = engine.get_top_holders_desc();
        assert_eq!(holders.len(), 2);
        assert_eq!(holders[0], (owner_a, 100));
        assert_eq!(holders[1], (owner_b, 90));
    }

    #[test]
    fn updates_existing_and_removes_zero_balance() {
        let mut engine = SupplySnapshotEngine::new(5);
        let owner_a = Pubkey::new_unique();
        let owner_b = Pubkey::new_unique();
        let account_a = Pubkey::new_unique();
        let account_b = Pubkey::new_unique();
        let mint = Pubkey::new_unique();

        engine.apply_token_account_update(1, account_a, owner_a, mint, 100);
        engine.apply_token_account_update(1, account_b, owner_b, mint, 50);

        engine.apply_token_account_update(2, account_a, owner_a, mint, 150);
        assert_eq!(engine.observed_supply(), 200);

        engine.apply_token_account_update(3, account_b, owner_b, mint, 0);
        assert_eq!(engine.observed_supply(), 150);

        let holders = engine.get_top_holders_desc();
        assert_eq!(holders, vec![(owner_a, 150)]);
    }
}
