use std::collections::BTreeMap;

use candid::{CandidType, Nat};
use serde::Deserialize;
use sns_treasury_manager::TransactionError;

use crate::{
    log,
    validation::{decode_nat_to_u64, ValidatedAsset},
};

#[derive(Clone, PartialEq, Eq, Hash, Debug, CandidType, Deserialize, PartialOrd, Ord)]
pub enum Party {
    TreasuryOwner,
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

#[derive(Default, CandidType, Deserialize, Clone)]
pub(crate) struct ValidatedBalanceForAsset {
    pub party_to_balance: BTreeMap<Party, u64>,
}

impl ValidatedBalanceForAsset {
    fn post_transaction(&mut self, entries: &Vec<LedgerEntry>) {
        for entry in entries {
            let balance = self
                .party_to_balance
                .entry(entry.account.clone())
                .or_insert(0);
            if entry.receives {
                *balance += entry.amount;
            } else {
                *balance -= entry.amount;
            }
        }
    }

    fn get_balance(&self, party: &Party) -> u64 {
        *self.party_to_balance.get(party).unwrap_or(&0)
    }

    fn stage_investment(&mut self, amount: u64, party: &Party) {
        self.party_to_balance
            .entry(party.clone())
            .and_modify(|balance| *balance += amount)
            .or_insert(amount);
    }

    fn unstage_investment(&mut self, amount: u64, party: &Party) -> Result<(), String> {
        match self.party_to_balance.get_mut(party) {
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
    fn refresh_party_balances(&mut self, party: Party, new_balance: u64) {
        if let Some(balance) = self.party_to_balance.get_mut(&party) {
            *balance = new_balance;
        } else {
            log(&format!("{:?} has not been registered", party));
            return;
        }
    }
}

#[derive(CandidType, Deserialize, Clone)]
pub(crate) struct ValidatedBalances {
    pub timestamp_ns: u64,
    pub asset_to_accounting: BTreeMap<ValidatedAsset, ValidatedBalanceForAsset>,
}

impl ValidatedBalances {
    pub fn new(assets: Vec<ValidatedAsset>, timestamp_ns: u64) -> Self {
        let asset_to_accounting = assets.iter().fold(BTreeMap::new(), |mut btm, asset| {
            btm.insert(asset.clone(), ValidatedBalanceForAsset::default());
            btm
        });

        Self {
            timestamp_ns,
            asset_to_accounting,
        }
    }

    fn add_asset(&mut self, asset: ValidatedAsset) {
        self.asset_to_accounting
            .insert(asset, ValidatedBalanceForAsset::default());
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
                let mut single_asset_accounting = ValidatedBalanceForAsset::default();
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

    pub fn refresh_asset(
        &mut self,
        asset_id: usize,
        old_asset: ValidatedAsset,
        new_asset: ValidatedAsset,
    ) {
        if let Some((mut asset, accounting)) = self.asset_to_accounting.remove_entry(&old_asset) {
            let ValidatedAsset::Token {
                symbol: new_symbol,
                ledger_fee_decimals: new_ledger_fee_decimals,
                ledger_canister_id: _,
            } = new_asset;

            if asset.set_symbol(new_symbol) {
                log(&format!(
                    "Changed asset_{} symbol from `{}` to `{}`.",
                    asset_id,
                    old_asset.symbol(),
                    new_symbol,
                ));
            }

            if asset.set_ledger_fee_decimals(new_ledger_fee_decimals) {
                log(&format!(
                    "Changed asset_{} ledger_fee_decimals from `{}` to `{}`.",
                    asset_id,
                    old_asset.ledger_fee_decimals(),
                    new_ledger_fee_decimals,
                ));
            }

            self.asset_to_accounting.insert(asset, accounting);
        } else {
            log(&format!(
                "Asset with symbol {} and ledger canister ID {} not found in the accounting",
                old_asset.symbol(),
                old_asset.ledger_canister_id()
            ));
            return;
        }
    }

    pub(crate) fn refresh_party_balances(
        &mut self,
        party: Party,
        asset: &ValidatedAsset,
        timstamp_ns: u64,
        new_balance: u64,
    ) {
        if let Some(single_asset_accounting) = self.asset_to_accounting.get_mut(asset) {
            single_asset_accounting.refresh_party_balances(party, new_balance);
            self.timestamp_ns = timstamp_ns;
        } else {
            log(&format!(
                "Coudln't refresh the balances. Asset {} not found",
                asset.symbol()
            ));
            return;
        }
    }
}
