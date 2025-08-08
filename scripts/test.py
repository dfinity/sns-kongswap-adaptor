#!/usr/bin/env python3

import argparse
import subprocess
import sys
from pathlib import Path

from common import run_command, get_project_paths, validate_project_structure, get_test_environment

def needs_rebuild(source_dir, target_file):
    """Check if target needs to be rebuilt based on source file modification times."""
    if not target_file.exists():
        return True
    
    target_mtime = target_file.stat().st_mtime
    
    # Check all Rust source files
    for rust_file in source_dir.rglob("*.rs"):
        if rust_file.stat().st_mtime > target_mtime:
            return True
    
    # Check Cargo.toml files
    for cargo_file in source_dir.rglob("Cargo.toml"):
        if cargo_file.stat().st_mtime > target_mtime:
            return True
    
    return False

def main():
    parser = argparse.ArgumentParser(description='Run tests for kongswap adaptor canister')
    parser.add_argument('--force-rebuild', action='store_true', 
                       help='Force rebuild even if WASM is up to date')
    parser.add_argument('--unit-only', action='store_true', 
                       help='Run only unit tests (skip integration tests)')
    parser.add_argument('--integration-only', action='store_true', 
                       help='Run only integration tests')
    parser.add_argument('--verbose', '-v', action='store_true', 
                       help='Run tests with verbose output')
    parser.add_argument('--test-name', type=str, 
                       help='Run specific test by name')
    parser.add_argument('--nocapture', action= 'store_true', help='Runs tests with `--no-capture` flag')

    args = parser.parse_args()
    
    # Get project paths
    paths = get_project_paths()
    validate_project_structure(paths)
    
    # Check if we need to rebuild (unless running unit tests only)
    if not args.unit_only:
        build_script = paths['project_dir'] / "scripts" / "build.py"
        
        if not build_script.exists():
            print(f"Error: Build script does not exist: {build_script}")
            sys.exit(1)
        
        target_wasm = paths['wasm_dir'] / paths['kongswap_canister_gz']
        source_dir = paths['project_dir'] / "kongswap_adaptor" / "src"
        
        if args.force_rebuild or needs_rebuild(source_dir, target_wasm):
            print("Rebuilding WASM...")
            run_command([sys.executable, str(build_script)], 
                       cwd=paths['project_dir'], 
                       description="Running build script")
        else:
            print("WASM is up to date, skipping rebuild")
    
    # Set up environment variables for tests
    env = get_test_environment(paths) if not args.unit_only else None
    
    # Build test command
    cargo_test_cmd = ["cargo", "test"]
    
    if args.verbose:
        cargo_test_cmd.append("--verbose")
    
    if args.unit_only:
        cargo_test_cmd.extend(["--bin", "kongswap-adaptor-canister"])
    elif args.integration_only:
        cargo_test_cmd.extend(["--test", "e2e"])
    
    if args.test_name:
        cargo_test_cmd.append(args.test_name)

    if args.nocapture:
        cargo_test_cmd.extend(["--", "--nocapture"])
    # Run tests
    print(f"Running tests: {' '.join(cargo_test_cmd)}")
    
    try:
        # Run without capturing output so we can see test progress
        result = subprocess.run(cargo_test_cmd, cwd=paths['project_dir'], env=env, check=True)
        print("All tests passed!")
    except subprocess.CalledProcessError as e:
        print(f"Tests failed with return code: {e.returncode}")
        sys.exit(1)

if __name__ == "__main__":
    main()