#!/usr/bin/env python3

import subprocess
import sys
from pathlib import Path

def run_command(cmd, cwd=None, description=None, env=None):
    """Run a command with error handling."""
    if description:
        print(f"{description}...")
    
    try:
        result = subprocess.run(cmd, cwd=cwd, env=env, check=True, 
                              capture_output=True, text=True)
        return result
    except subprocess.CalledProcessError as e:
        print(f"Error running command: {' '.join(cmd) if isinstance(cmd, list) else cmd}")
        print(f"Return code: {e.returncode}")
        if e.stdout:
            print(f"Stdout: {e.stdout}")
        if e.stderr:
            print(f"Stderr: {e.stderr}")
        sys.exit(1)

def run_ic_wasm(args, cwd=None):
    """Run ic-wasm command with error handling."""
    cmd = ["ic-wasm"] + args
    return run_command(cmd, cwd=cwd)

def get_project_paths():
    """Get common project paths."""
    HOME = Path.home()
    PROJECT_DIR = HOME / "sns-kongswap-adaptor"
    WASM_DIR = PROJECT_DIR / "target" / "wasm32-unknown-unknown" / "release"
    CANDID = PROJECT_DIR / "kongswap_adaptor" / "kongswap-adaptor.did"
    
    return {
        'home': HOME,
        'project_dir': PROJECT_DIR,
        'wasm_dir': WASM_DIR,
        'candid': CANDID,
        'kongswap_canister': "kongswap-adaptor-canister.wasm",
        'kongswap_canister_gz': "kongswap-adaptor-canister.wasm.gz",
    }

def validate_project_structure(paths):
    """Validate that the project structure exists."""
    if not paths['project_dir'].exists():
        print(f"Error: Project directory does not exist: {paths['project_dir']}")
        sys.exit(1)
    
    return True

def get_test_environment(paths):
    """Get environment variables for tests."""
    import os
    
    env = os.environ.copy()
    env.update({
        "KONGSWAP_ADAPTOR_CANISTER_WASM_PATH": str(paths['wasm_dir'] / paths['kongswap_canister_gz']),
        "IC_ICRC1_LEDGER_WASM_PATH": str(paths['home'] / "ic" / "ledger_canister.wasm.gz"),
        "KONG_BACKEND_CANISTER_WASM_PATH": str(paths['home'] / "ic" / "rs" / "nervous_system" / "integration_tests" / "kong_backend.wasm"),
        "MAINNET_ICP_LEDGER_CANISTER_WASM_PATH": str(paths['home'] / "ic" / "artifacts" / "canisters" / "ledger-canister.wasm.gz"),
    })
    
    return env