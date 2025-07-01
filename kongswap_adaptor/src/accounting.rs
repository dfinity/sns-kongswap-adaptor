use std::collections::HashMap;

use candid::{CandidType, Nat};
use serde::Deserialize;
use sns_treasury_manager::TransactionError;

use crate::validation::{decode_nat_to_u64, ValidatedAsset};

#[derive(Clone, PartialEq, Eq, Hash, Debug, CandidType, Deserialize)]
pub enum Party {
    Sns,
    TreasuryManager,
    External,
    LedgerFee,
}

#[derive(Clone, CandidType, Deserialize)]
pub struct LedgerEntry {
    pub account: Party,
    pub amount: u64,
    pub receives: bool,
}

pub(crate) fn create_ledger_entries(
    src_account: Party,
    dst_account: Party,
    amount: Nat,
    fee: Nat,
) -> Result<Vec<LedgerEntry>, TransactionError> {
    let amount = decode_nat_to_u64(amount).map_err(TransactionError::Postcondition)?;
    let fee = decode_nat_to_u64(fee).map_err(TransactionError::Postcondition)?;

    Ok(vec![
        LedgerEntry {
            account: src_account,
            amount,
            receives: false,
        },
        LedgerEntry {
            account: dst_account,
            amount: amount - fee,
            receives: true,
        },
        LedgerEntry {
            account: Party::LedgerFee,
            amount: fee,
            receives: true,
        },
    ])
}

#[derive(Default, CandidType, Deserialize)]
struct SingleAssetAccounting {
    balances: HashMap<Party, u64>,
    journal: Vec<Vec<LedgerEntry>>,
}

impl SingleAssetAccounting {
    fn post_transaction(&mut self, entries: &Vec<LedgerEntry>) {
        // Apply entries
        for entry in entries {
            let balance = self.balances.entry(entry.account.clone()).or_insert(0);
            if entry.receives {
                *balance += entry.amount;
            } else {
                *balance -= entry.amount;
            }
        }

        self.journal.push(entries.clone());
    }

    fn get_balance(&self, party: &Party) -> u64 {
        *self.balances.get(party).unwrap_or(&0)
    }

    fn stage_investment(&mut self, amount: u64, party: &Party) {
        self.balances
            .entry(party.clone())
            .and_modify(|balance| *balance += amount)
            .or_insert(amount);
    }

    fn unstage_investment(&mut self, amount: u64, party: &Party) -> Result<(), String> {
        match self.balances.get_mut(party) {
            Some(balance) if *balance >= amount => {
                *balance -= amount;
                Ok(())
            }
            Some(_) => Err(format!(
                "{:?} does not have enough staged investments (requested: {})",
                party, amount
            )),
            None => Err(format!("{:?} has no staged investments", party)),
        }
    }
}

#[derive(CandidType, Deserialize)]
pub(crate) struct MultiAssetAccounting {
    asset_to_accounting: HashMap<ValidatedAsset, SingleAssetAccounting>,
}

impl MultiAssetAccounting {
    pub fn new(assets: Vec<ValidatedAsset>) -> Self {
        let asset_to_accounting = assets.iter().fold(HashMap::new(), |mut hm, asset| {
            hm.insert(asset.clone(), SingleAssetAccounting::default());
            hm
        });

        Self {
            asset_to_accounting,
        }
    }

    fn add_asset(&mut self, asset: ValidatedAsset) {
        self.asset_to_accounting
            .insert(asset, SingleAssetAccounting::default());
    }

    pub fn get_balance(&self, asset: &ValidatedAsset, party: &Party) -> Result<u64, String> {
        let Some(single_asset_accounting) = self.asset_to_accounting.get(asset) else {
            return Err(format!(
                "Asset {} is not added to the accounting.",
                asset.symbol()
            ));
        };

        Ok(single_asset_accounting.get_balance(party))
    }

    pub fn post_asset_transaction(
        &mut self,
        asset: &ValidatedAsset,
        entries: &Vec<LedgerEntry>,
    ) -> Result<(), String> {
        let Some(single_asset_accounting) = self.asset_to_accounting.get_mut(asset) else {
            return Err(format!(
                "Asset {} is not added to the accounting.",
                asset.symbol()
            ));
        };

        single_asset_accounting.post_transaction(entries);
        Ok(())
    }

    pub fn stage_asset_investment(
        &mut self,
        asset: &ValidatedAsset,
        amount: Nat,
        party: &Party,
    ) -> Result<(), String> {
        let amount = decode_nat_to_u64(amount)?;

        self.asset_to_accounting
            .entry(*asset)
            .and_modify(|accounting| accounting.stage_investment(amount, party))
            .or_insert({
                let mut single_asset_accounting = SingleAssetAccounting::default();
                single_asset_accounting.stage_investment(amount, party);
                single_asset_accounting
            });

        Ok(())
    }

    pub fn unstage_asset_investment(
        &mut self,
        asset: &ValidatedAsset,
        amount: Nat,
        party: &Party,
    ) -> Result<(), String> {
        let amount = decode_nat_to_u64(amount)?;

        if let Some(single_asset_accounting) = self.asset_to_accounting.get_mut(asset) {
            single_asset_accounting.unstage_investment(amount, party)?;
        } else {
            return Err(format!(
                "Asset {} not found for {:?}. Make sure that you have staged before",
                asset.symbol(),
                party
            ));
        }

        Ok(())
    }
}
