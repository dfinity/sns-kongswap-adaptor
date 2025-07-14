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
    import os
    
    # In CI, use the current working directory
    if os.environ.get('GITHUB_ACTIONS'):
        PROJECT_DIR = Path.cwd()
        IC_DIR = PROJECT_DIR.parent / "ic"
        
        # Debug: Print the detected paths
        print(f"[CI] Current working directory: {PROJECT_DIR}")
        print(f"[CI] Project contents: {list(PROJECT_DIR.iterdir())}")
        print(f"[CI] IC directory: {IC_DIR}")
        print(f"[CI] IC contents: {list(IC_DIR.iterdir())}")
        
        # Verify workspace structure
        cargo_toml = PROJECT_DIR / "Cargo.toml"
        if cargo_toml.exists():
            print(f"[CI] Found Cargo.toml at: {cargo_toml}")
        else:
            print(f"[CI] ERROR: No Cargo.toml found at: {cargo_toml}")
            
        kongswap_dir = PROJECT_DIR / "kongswap_adaptor"
        if kongswap_dir.exists():
            print(f"[CI] Found kongswap_adaptor directory at: {kongswap_dir}")
        else:
            print(f"[CI] ERROR: No kongswap_adaptor directory found at: {kongswap_dir}")
            
    else:
        # Local development
        PROJECT_DIR = Path.home() / "sns-kongswap-adaptor"
        IC_DIR = Path.home() / "ic"
    
    WASM_DIR = PROJECT_DIR / "target" / "wasm32-unknown-unknown" / "release"
    CANDID = PROJECT_DIR / "kongswap_adaptor" / "kongswap-adaptor.did"
    
    return {
        'ic_dir': IC_DIR,
        'project_dir': PROJECT_DIR,
        'wasm_dir': WASM_DIR,
        'candid': CANDID,
        'kongswap_canister': "kongswap-adaptor-canister.wasm",
        'kongswap_canister_gz': "kongswap-adaptor-canister.wasm.gz",
        'kong_version': "4bf8f99df53dbd34bef0e55ab6364d85bb31c71a",
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
    
    # Download kong backend if needed
    download_kong_backend(paths)
    
    artifacts_dir = paths['project_dir'] / "ic-artifacts"
    
    env = os.environ.copy()
    env.update({
        "KONGSWAP_ADAPTOR_CANISTER_WASM_PATH": str(paths['wasm_dir'] / paths['kongswap_canister_gz']),
        "IC_ICRC1_LEDGER_WASM_PATH": str(artifacts_dir / "mainnet-sns-ledger.wasm.gz"),
        "MAINNET_ICP_LEDGER_CANISTER_WASM_PATH": str(artifacts_dir / "mainnet-icp-ledger.wasm.gz"),
        "KONG_BACKEND_CANISTER_WASM_PATH": str(artifacts_dir / "kong_backend.wasm.gz"),
    })
    
    return env

def download_mainnet_canisters(paths):
    """Download mainnet canister WASMs from dfinity CDN using mainnet-canister-revisions.json."""
    import json
    import urllib.request
    
    # Validate IC directory exists
    ic_dir = paths['ic_dir']
    
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

def load_config(paths):
    """Load configuration from config.json."""
    import json
    
    config_file = paths['project_dir'] / "config.json"
    if not config_file.exists():
        print(f"Error: Configuration file not found: {config_file}")
        sys.exit(1)
    
    try:
        with open(config_file, 'r') as f:
            config = json.load(f)
    except Exception as e:
        print(f"Error reading config.json: {e}")
        sys.exit(1)
    
    # Validate required configuration keys
    if "dependencies" not in config:
        print("Error: Missing 'dependencies' section in config.json")
        sys.exit(1)
    
    if "kong_backend" not in config["dependencies"]:
        print("Error: Missing 'kong_backend' configuration in dependencies")
        sys.exit(1)
    
    kong_config = config["dependencies"]["kong_backend"]
    required_keys = ["version", "url_template"]
    
    for key in required_keys:
        if key not in kong_config:
            print(f"Error: Missing required key '{key}' in kong_backend configuration")
            sys.exit(1)
    
    return config

def download_kong_backend(paths):
    """Download kong_backend.wasm.gz from GitHub releases."""
    import urllib.request
    
    config = load_config(paths)
    kong_config = config["dependencies"]["kong_backend"]
    
    artifacts_dir = paths['project_dir'] / "ic-artifacts"
    artifacts_dir.mkdir(exist_ok=True)
    
    dest_path = artifacts_dir / "kong_backend.wasm.gz"
    
    # Skip if already exists
    if dest_path.exists():
        print(f"Kong Backend already exists: {dest_path}")
        return
    
    version = kong_config["version"]
    download_url = kong_config["url_template"].format(version=version)
    
    print(f"Downloading Kong Backend from GitHub...")
    print(f"  Version: {version}")
    print(f"  URL: {download_url}")
    
    try:
        urllib.request.urlretrieve(download_url, dest_path)
        print(f"  Downloaded Kong Backend -> {dest_path}")
    except Exception as e:
        print(f"Error downloading Kong Backend: {e}")
        sys.exit(1)
