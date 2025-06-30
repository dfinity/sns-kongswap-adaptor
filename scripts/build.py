#!/usr/bin/env python3
# filepath: /home/arshavir/sns-kongswap-adaptor/scripts/build.py

import gzip
import os
import shutil
import subprocess
import sys
from pathlib import Path

def run_command(cmd, cwd=None, description=None):
    """Run a command with error handling."""
    if description:
        print(f"{description}...")
    
    try:
        result = subprocess.run(cmd, cwd=cwd, check=True, 
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

def compress_file(input_path, output_path):
    """Compress a file using gzip with maximum compression."""
    with open(input_path, 'rb') as f_in:
        with gzip.open(output_path, 'wb', compresslevel=9) as f_out:
            shutil.copyfileobj(f_in, f_out)

def main():
    # Configuration
    KONGSWAP_ADAPTOR_CANISTER = "kongswap-adaptor-canister.wasm"
    HOME = Path.home()
    PROJECT_DIR = HOME / "sns-kongswap-adaptor"
    WASM_DIR = PROJECT_DIR / "target" / "wasm32-unknown-unknown" / "release"
    CANDID = PROJECT_DIR / "kongswap_adaptor" / "kongswap-adaptor.did"
    
    # Validate project directory exists
    if not PROJECT_DIR.exists():
        print(f"Error: Project directory does not exist: {PROJECT_DIR}")
        sys.exit(1)
    
    # Build the project
    print("Building Rust project...")
    cargo_cmd = [
        "cargo", "build",
        "--target", "wasm32-unknown-unknown",
        "--release",
        "--bin", "kongswap-adaptor-canister"
    ]
    
    run_command(cargo_cmd, cwd=PROJECT_DIR, description="Running cargo build")
    
    # Validate paths after build
    if not WASM_DIR.exists():
        print(f"Error: WASM directory does not exist after build: {WASM_DIR}")
        sys.exit(1)
    
    if not CANDID.exists():
        print(f"Error: Candid file does not exist: {CANDID}")
        sys.exit(1)
    
    # File paths
    original_wasm = WASM_DIR / KONGSWAP_ADAPTOR_CANISTER
    augmented_wasm = WASM_DIR / f"augmented-{KONGSWAP_ADAPTOR_CANISTER}"
    final_compressed = WASM_DIR / f"{KONGSWAP_ADAPTOR_CANISTER}.gz"
    
    if not original_wasm.exists():
        print(f"Error: Original WASM file does not exist after build: {original_wasm}")
        sys.exit(1)
    
    try:
        print("Adding metadata to WASM...")
        run_ic_wasm([
            "-o", str(augmented_wasm),
            str(original_wasm),
            "metadata", "-v", "public",
            "candid:service", "-f", str(CANDID)
        ])
        
        print("Optimizing WASM...")
        run_ic_wasm([
            "-o", str(augmented_wasm),
            str(augmented_wasm),
            "optimize", "--keep-name-section", "Os"
        ])
        
        print("Compressing WASM...")
        compress_file(augmented_wasm, final_compressed)
        
        # Clean up intermediate file
        augmented_wasm.unlink()
        
        print(f"Build complete! Output: {final_compressed}")
        print(f"File size: {final_compressed.stat().st_size:,} bytes")
        
    except Exception as e:
        print(f"Unexpected error: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()