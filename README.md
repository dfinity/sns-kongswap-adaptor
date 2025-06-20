# Building the project

## Prerequisites

```
sudo apt-get install liblmdb-dev clang
```

### Dependencies

```
git clone https://github.com/KongSwap/kong.git
git clone https://github.com/dfinity/ic.git
git clone https://github.com/dfinity/sns-treasury-manager.git
cd sns-treasury-manager
```

### Build kongswap-adaptor-canister.wasm

Release version:

```
cargo build \
    --target wasm32-unknown-unknown \
    --release \
    --bin kongswap-adaptor-canister
```

Test version:

```
cargo build \
    --target wasm32-unknown-unknown \
    --bin kongswap-adaptor-canister
```

# Testing

Assuming the required repositories were cloned into `$HOME`:

```
export KONGSWAP_ADAPTOR_CANISTER_WASM_PATH="$HOME/sns-treasury-manager/target/wasm32-unknown-unknown/debug/kongswap-adaptor-canister.wasm"
export IC_ICRC1_LEDGER_WASM_PATH="$HOME/ic/ledger_canister.wasm.gz"
export KONG_BACKEND_CANISTER_WASM_PATH="$HOME/ic/rs/nervous_system/integration_tests/kong_backend.wasm"
export MAINNET_ICP_LEDGER_CANISTER_WASM_PATH="$HOME/ic/artifacts/canisters/ledger-canister.wasm.gz"
clear && cargo test
```

