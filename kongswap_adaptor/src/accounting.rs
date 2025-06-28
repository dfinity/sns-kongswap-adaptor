use std::collections::HashMap;

use candid::Nat;
use sns_treasury_manager::TransactionError;

use crate::validation::{decode_nat_to_u64, ValidatedAsset};

#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Party {
    Sns,
    TreasuryManager,
    External,
    LedgerFee,
}

#[derive(Clone)]
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

#[derive(Default)]
struct SingleAssetAccounting {
    balances: HashMap<Party, i64>,
    journal: Vec<Vec<LedgerEntry>>,
}

impl SingleAssetAccounting {
    fn post_transaction(&mut self, entries: &Vec<LedgerEntry>) {
        // Apply entries
        for entry in entries {
            let balance = self.balances.entry(entry.account.clone()).or_insert(0);
            if entry.receives {
                *balance += entry.amount as i64;
            } else {
                *balance -= entry.amount as i64;
            }
        }

        self.journal.push(entries.clone());
    }

    fn get_balance(&self, party: &Party) -> i64 {
        *self.balances.get(party).unwrap_or(&0)
    }
}

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

    pub fn get_balance(&self, asset: &ValidatedAsset, party: &Party) -> Result<i64, String> {
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
}
