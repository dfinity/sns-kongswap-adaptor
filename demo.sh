#!/usr/bin/env bash
dfx stop
set -e
trap 'dfx stop' EXIT

echo "===========SETUP========="
echo "Deploying ICP Ledger canister..."
dfx start --background --clean
# dfx identity new alice_icp_transfer --storage-mode plaintext --force

export MINTER_ACCOUNT_ID=$(dfx --identity anonymous ledger account-id)
export DEFAULT_ACCOUNT_ID=$(dfx ledger account-id)

dfx deploy icp_ledger_canister --argument "
  (variant {
    Init = record {
      minting_account = \"$MINTER_ACCOUNT_ID\";
      initial_values = vec {
        record {
          \"$DEFAULT_ACCOUNT_ID\";
          record {
            e8s = 10_000_000_000 : nat64;
          };
        };
      };
      send_whitelist = vec {};
      transfer_fee = opt record {
        e8s = 10_000 : nat64;
      };
      token_symbol = opt \"LICP\";
      token_name = opt \"Local ICP\";
    }
  })
"
dfx canister call icp_ledger_canister account_balance '(record { account = '$(python3 -c 'print("vec{" + ";".join([str(b) for b in bytes.fromhex("'$DEFAULT_ACCOUNT_ID'")]) + "}")')'})'

echo "Deploying SNS Ledger canister..."
dfx deploy sns_ledger_canister --argument "
  (variant {
    Init = record {
      minting_account = \"$MINTER_ACCOUNT_ID\";
      initial_values = vec {
        record {
          \"$DEFAULT_ACCOUNT_ID\";
          record {
            e8s = 10_000_000_000 : nat64;
          };
        };
      };
      send_whitelist = vec {};
      transfer_fee = opt record {
        e8s = 10_000 : nat64;
      };
      token_symbol = opt \"LSNS\";
      token_name = opt \"Local SNS\";
    }
  })
"
dfx canister call sns_ledger_canister account_balance '(record { account = '$(python3 -c 'print("vec{" + ";".join([str(b) for b in bytes.fromhex("'$DEFAULT_ACCOUNT_ID'")]) + "}")')'})'

echo "Deploying Kong Backend canister..."

dfx deploy kong_backend

echo "===========SETUP DONE==========="
export ICP_LEDGER_CANISTER=$(dfx canister id icp_ledger_canister)
export SNS_LEDGER_CANISTER=$(dfx canister id sns_ledger_canister)
export MINTER_PRINCIPAL=$(dfx identity --identity anonymous get-principal)

echo "ICP Ledger Canister ID: $ICP_LEDGER_CANISTER"
echo "SNS Ledger Canister ID: $SNS_LEDGER_CANISTER" 
echo "Minter Account ID: $MINTER_ACCOUNT_ID"
echo "Minter prinicipal: $(dfx identity --identity anonymous get-principal)"

echo "Deploying SNS Kongswap Adaptor canister..."
dfx deploy sns_kongswap_adaptor --argument "
  (variant {
    Init = record {
      allowances = vec {
        record {
          asset = variant {
            Token = record {
              ledger_fee_decimals = 10_000 : nat;
              ledger_canister_id = principal \"$ICP_LEDGER_CANISTER\";
              symbol = \"LICP\";
            }
          };
          amount_decimals = 100_000 : nat;
          owner_account = record {
            owner = principal \"$MINTER_PRINCIPAL\";
            subaccount = null;
          };
        };
        record {
          asset = variant {
            Token = record {
              ledger_fee_decimals = 10_000 : nat;
              ledger_canister_id = principal \"$SNS_LEDGER_CANISTER\";
              symbol = \"LSNS\";
            }
          };
          amount_decimals = 100_000 : nat;
          owner_account = record {
            owner = principal \"$MINTER_PRINCIPAL\";
            subaccount = null;
          };
        }
      };
    }
  })
"

# TOKENS_TRANSFER_ACCOUNT_ID="$(dfx ledger account-id --of-canister icp_transfer_backend)"
# TOKENS_TRANSFER_ACCOUNT_ID_BYTES="$(python3 -c 'print("vec{" + ";".join([str(b) for b in bytes.fromhex("'$TOKENS_TRANSFER_ACCOUNT_ID'")]) + "}")')"
# dfx canister call icp_ledger_canister transfer "(record { to=${TOKENS_TRANSFER_ACCOUNT_ID_BYTES}; amount=record { e8s=100_000 }; fee=record { e8s=10_000 }; memo=0:nat64; }, )"
# 
# dfx canister call icp_transfer_backend transfer "(record { amount=record { e8s=5 }; to_principal=principal \"$(dfx identity get-principal)\" },)"

echo "DONE"