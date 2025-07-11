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
    
    # Download mainnet canisters if needed
    download_mainnet_canisters(paths)
    
    artifacts_dir = paths['project_dir'] / "ic-artifacts"
    
    env = os.environ.copy()
    env.update({
        "KONGSWAP_ADAPTOR_CANISTER_WASM_PATH": str(paths['wasm_dir'] / paths['kongswap_canister_gz']),
        "IC_ICRC1_LEDGER_WASM_PATH": str(artifacts_dir / "mainnet-sns-ledger.wasm.gz"),
        "MAINNET_ICP_LEDGER_CANISTER_WASM_PATH": str(artifacts_dir / "mainnet-icp-ledger.wasm.gz"),
        "KONG_BACKEND_CANISTER_WASM_PATH": str(paths['home'] / "kong" / "target" / "wasm32-unknown-unknown" / "release" / "kong_backend.wasm"),
    })
    
    return env

def download_mainnet_canisters(paths):
    """Download mainnet canister WASMs from dfinity CDN using mainnet-canister-revisions.json."""
    import json
    import urllib.request
    
    # Validate IC directory exists
    ic_dir = paths['home'] / "ic"
    
    if not ic_dir.exists():
        print(f"Error: IC directory does not exist: {ic_dir}")
        print("Please clone the IC repository first: git clone https://github.com/dfinity/ic.git")
        sys.exit(1)
    
    # Read mainnet canister revisions
    revisions_file = ic_dir / "mainnet-canister-revisions.json"
    if not revisions_file.exists():
        print(f"Error: mainnet-canister-revisions.json not found: {revisions_file}")
        sys.exit(1)
    
    try:
        with open(revisions_file, 'r') as f:
            revisions = json.load(f)
    except Exception as e:
        print(f"Error reading mainnet-canister-revisions.json: {e}")
        sys.exit(1)
    
    # Create artifacts directory
    artifacts_dir = paths['project_dir'] / "ic-artifacts"
    artifacts_dir.mkdir(exist_ok=True)
    
    # Define canisters to find and download
    canister_mapping = {
        "sns_ledger": {
            "cdn_filename": "ic-icrc1-ledger.wasm.gz",
            "local_filename": "mainnet-sns-ledger.wasm.gz",
            "name": "SNS Ledger"
        },
        "ledger": {
            "cdn_filename": "ledger-canister.wasm.gz", 
            "local_filename": "mainnet-icp-ledger.wasm.gz",
            "name": "ICP Ledger"
        }
    }
    
    for canister_key, config in canister_mapping.items():
        dest_path = artifacts_dir / config["local_filename"]
        
        # Skip if already exists
        if dest_path.exists():
            print(f"{config['name']} already exists: {dest_path}")
            continue
        
        # Find canister in revisions by key
        if canister_key not in revisions:
            print(f"Error: {config['name']} (key: {canister_key}) not found in mainnet-canister-revisions.json")
            print(f"Available keys: {list(revisions.keys())}")
            sys.exit(1)
        
        canister_info = revisions[canister_key]
        
        # Get the IC commit ID (a.k.a., revision) and construct download URL
        revision = canister_info.get("rev")
        if not revision:
            print(f"Error: No field `rev` found for {config['name']} in mainnet-canister-revisions.json")
            sys.exit(1)
        
        download_url = f"https://download.dfinity.systems/ic/{revision}/canisters/{config['cdn_filename']}"
        
        print(f"Downloading {config['name']} from CDN...")
        print(f"  SHA256: {canister_info.get('sha256', 'N/A')}")
        print(f"  IC commit: {revision}")
        print(f"  URL: {download_url}")

        try:
            urllib.request.urlretrieve(download_url, dest_path)
            print(f"  Downloaded {config['name']} -> {dest_path}")
            
        except Exception as e:
            print(f"Error downloading {config['name']}: {e}")
            sys.exit(1)
    
    print(f"All mainnet canisters downloaded to {artifacts_dir}")
