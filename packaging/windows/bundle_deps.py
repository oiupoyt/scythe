#!/usr/bin/env python3
import os
import sys
import glob
import shutil
import subprocess

def main():
    bundle_dir = sys.argv[1] if len(sys.argv) > 1 else "dist/bundle"
    if not os.path.exists(bundle_dir):
        print(f"Error: Bundle directory '{bundle_dir}' not found.")
        sys.exit(1)

    print(f"Resolving dynamic Windows dependencies for {bundle_dir}...")
    
    # 1. Locate MinGW bin directory containing FFmpeg DLLs
    candidate_dirs = [
        os.path.dirname(os.path.abspath(sys.executable)),
        "/mingw64/bin",
        "C:/msys64/mingw64/bin",
        "D:/msys64/mingw64/bin",
    ]
    mingw_bin = None
    for cand in candidate_dirs:
        if os.path.isdir(cand) and glob.glob(os.path.join(cand, "avcodec-*.dll")):
            mingw_bin = cand
            break

    if not mingw_bin:
        for cand in candidate_dirs:
            if os.path.isdir(cand):
                mingw_bin = cand
                break

    if not mingw_bin:
        mingw_bin = "/mingw64/bin"

    print(f"Using MinGW bin directory: {mingw_bin}")

    patterns = [
        "avcodec-*.dll",
        "avformat-*.dll",
        "avutil-*.dll",
        "swresample-*.dll",
        "swscale-*.dll",
        "avfilter-*.dll",
        "avdevice-*.dll",
        "libwinpthread-*.dll",
        "libgcc_s_seh-*.dll",
        "libstdc++-*.dll",
        "zlib*.dll",
    ]

    for pat in patterns:
        for match in glob.glob(os.path.join(mingw_bin, pat)):
            dest = os.path.join(bundle_dir, os.path.basename(match))
            if not os.path.exists(dest):
                shutil.copy2(match, dest)
                print(f"Explicitly copied: {os.path.basename(match)}")

    # 2. Recursive dependency discovery via ldd
    def get_ldd_deps(filepath):
        deps = []
        try:
            out = subprocess.check_output(["ldd", filepath], text=True, stderr=subprocess.DEVNULL)
            for line in out.splitlines():
                line = line.strip()
                if "mingw64/bin" in line.lower():
                    tokens = line.split()
                    for token in tokens:
                        if "mingw64/bin" in token.lower() and token.lower().endswith(".dll"):
                            if os.path.exists(token):
                                deps.append(token)
                            elif mingw_bin and os.path.exists(os.path.join(mingw_bin, os.path.basename(token))):
                                deps.append(os.path.join(mingw_bin, os.path.basename(token)))
        except Exception:
            pass
        return deps

    visited = set()
    added_count = 0

    while True:
        candidates = [
            os.path.join(bundle_dir, f)
            for f in os.listdir(bundle_dir)
            if (f.lower().endswith(".exe") or f.lower().endswith(".dll")) and f not in visited
        ]
        if not candidates:
            break

        new_found = False
        for c in candidates:
            visited.add(os.path.basename(c))
            for dep in get_ldd_deps(c):
                dll_name = os.path.basename(dep)
                dest = os.path.join(bundle_dir, dll_name)
                if not os.path.exists(dest):
                    shutil.copy2(dep, dest)
                    added_count += 1
                    new_found = True
                    print(f"Discovered dependency: {dll_name} (from {os.path.basename(c)})")

        if not new_found:
            break

    # 3. Validation
    all_files = os.listdir(bundle_dir)
    has_avcodec = any("avcodec" in f.lower() for f in all_files)
    if not has_avcodec:
        print("ERROR: avcodec-*.dll was NOT found in bundle directory!")
        sys.exit(1)

    print(f"Successfully assembled bundle in '{bundle_dir}' with {len(all_files)} total files.")

if __name__ == "__main__":
    main()
