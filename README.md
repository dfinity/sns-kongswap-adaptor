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
./scripts/test.py
```

