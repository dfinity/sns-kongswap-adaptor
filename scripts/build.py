#!/usr/bin/env python3

import gzip
import shutil
import sys
from pathlib import Path

from common import run_command, run_ic_wasm, get_project_paths, validate_project_structure

def compress_file(input_path, output_path):
    """Compress a file using gzip with maximum compression."""
    with open(input_path, 'rb') as f_in:
        with gzip.open(output_path, 'wb', compresslevel=9) as f_out:
            shutil.copyfileobj(f_in, f_out)

def main():
    # Get project paths
    paths = get_project_paths()
    validate_project_structure(paths)
    
    # Build the project
    print("Building Rust project...")
    cargo_cmd = [
        "cargo", "build",
        "--target", "wasm32-unknown-unknown",
        "--release",
        "--bin", "kongswap-adaptor-canister"
    ]
    
    run_command(cargo_cmd, cwd=paths['project_dir'], description="Running cargo build")
    
    # Validate paths after build
    if not paths['wasm_dir'].exists():
        print(f"Error: WASM directory does not exist after build: {paths['wasm_dir']}")
        sys.exit(1)
    
    if not paths['candid'].exists():
        print(f"Error: Candid file does not exist: {paths['candid']}")
        sys.exit(1)
    
    # File paths
    original_wasm = paths['wasm_dir'] / paths['kongswap_canister']
    augmented_wasm = paths['wasm_dir'] / f"augmented-{paths['kongswap_canister']}"
    final_compressed = paths['wasm_dir'] / paths['kongswap_canister_gz']
    
    if not original_wasm.exists():
        print(f"Error: Original WASM file does not exist after build: {original_wasm}")
        sys.exit(1)
    
    try:
        print("Adding metadata to WASM...")
        run_ic_wasm([
            "-o", str(augmented_wasm),
            str(original_wasm),
            "metadata", "-v", "public",
            "candid:service", "-f", str(paths['candid'])
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