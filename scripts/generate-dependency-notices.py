"""Generate notices from target-filtered Cargo metadata, without local paths."""
import argparse
import json
from pathlib import Path


def generate(metadata):
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    root = next(package["id"] for package in packages.values() if package["name"] == "tide-music")
    pending, selected = [root], set()
    while pending:
        package_id = pending.pop()
        if package_id in selected:
            continue
        selected.add(package_id)
        for dependency in nodes[package_id]["deps"]:
            if any(kind["kind"] != "dev" for kind in dependency["dep_kinds"]):
                pending.append(dependency["pkg"])
    lines = ["# Tide dependency notices", "",
             "Generated from Cargo metadata for the Windows build graph, including build tools.",
             "Some listed packages are build-only and are not included in the executable.",
             "License alternatives below reproduce upstream metadata; Slint is used under",
             "its Royalty-free Desktop, Mobile, and Web Applications License.",
             "Playback tools and Python packages are downloaded separately and are not bundled.", ""]
    for package in sorted((packages[key] for key in selected if packages[key]["source"]),
                          key=lambda item: (item["name"], item["version"])):
        name, version = package["name"], package["version"]
        directory = Path(package["manifest_path"]).parent
        terms = package.get("license") or "See upstream license file"
        lines.extend([f"## {name} {version}", "", f"License metadata: {terms}",
                      f"Source: https://crates.io/crates/{name}/{version}", ""])
        candidates = set()
        explicit = package.get("license_file")
        if explicit:
            candidates.add(directory / explicit)
        for parent in (directory, directory / "LICENSES"):
            if parent.is_dir():
                for path in parent.iterdir():
                    if path.is_file() and (parent.name == "LICENSES" or path.name.upper().startswith(("LICENSE", "COPYING", "NOTICE", "COPYRIGHT"))):
                        candidates.add(path)
        for path in sorted(candidates):
            if path.is_symlink() or not path.resolve().is_relative_to(directory.resolve()):
                raise ValueError(f"License text escapes its package for {name} {version}")
            if not path.is_file() or path.stat().st_size > 300_000:
                raise ValueError(f"Missing or oversized license text for {name} {version}")
            content = path.read_text(encoding="utf-8-sig")
            if "\0" in content:
                raise ValueError(f"Non-text license for {name} {version}")
            lines.extend([f"### {path.name}", "", content.strip(), ""])
        if not candidates:
            lines.extend(["No separate license/notice file was included in this published crate.", ""])
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("metadata", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    result = generate(json.loads(args.metadata.read_text(encoding="utf-8-sig")))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(result, encoding="utf-8")
    print("Dependency notices generated; local metadata paths are not included.")


if __name__ == "__main__":
    main()
