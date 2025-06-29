# Building the project

## Prerequisites

```
sudo apt-get install liblmdb-dev clang python3
cargo install ic-wasm
```

### Dependencies

```
git clone https://github.com/KongSwap/kong.git
git clone https://github.com/dfinity/ic.git
git clone https://github.com/dfinity/sns-kongswap-adaptor.git
cd sns-kongswap-adaptor
```

### Build kongswap-adaptor-canister.wasm

Release version:

```
./scripts/build.py
```

# Testing

Assuming the required repositories were cloned into `$HOME`:

```
./scripts/build.py
export KONGSWAP_ADAPTOR_CANISTER_WASM_PATH="$HOME/sns-kongswap-adaptor/target/wasm32-unknown-unknown/release/kongswap-adaptor-canister.wasm.gz"
export IC_ICRC1_LEDGER_WASM_PATH="$HOME/ic/ledger_canister.wasm.gz"
export KONG_BACKEND_CANISTER_WASM_PATH="$HOME/ic/rs/nervous_system/integration_tests/kong_backend.wasm"
export MAINNET_ICP_LEDGER_CANISTER_WASM_PATH="$HOME/ic/artifacts/canisters/ledger-canister.wasm.gz"
clear && cargo test
```

