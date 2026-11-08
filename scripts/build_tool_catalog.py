import argparse
import json
import shutil
import tomllib
from pathlib import Path
from urllib.parse import quote


ROOT = Path(__file__).resolve().parents[1]
TOOLS_DIR = ROOT / "tools"
SITE_DIR = ROOT / "site"
REPOSITORY = "ktrxservor/KTRX"
BRANCH = "main"
ICON_FILENAME = "icon.png"
ICONS_OUTPUT_DIR = "icons"


def repository_url(path: str, *, raw: bool = False) -> str:
    encoded_path = quote(path, safe="/")
    if raw:
        return f"https://github.com/{REPOSITORY}/raw/refs/heads/{BRANCH}/{encoded_path}"
    return f"https://github.com/{REPOSITORY}/blob/{BRANCH}/{encoded_path}"


def load_tools() -> list[dict[str, object]]:
    tools = []
    for manifest_path in sorted(TOOLS_DIR.glob("*/manifest.toml")):
        with manifest_path.open("rb") as manifest_file:
            manifest = tomllib.load(manifest_file)

        name = manifest.get("name")
        tool_id = manifest.get("id")
        description = manifest.get("description")
        version = manifest.get("version")
        permissions = manifest.get("permissions", [])
        if not all(isinstance(value, str) and value for value in (name, tool_id, description, version)):
            raise ValueError(f"{manifest_path.relative_to(ROOT)}: required metadata is missing or invalid")
        if not isinstance(permissions, list) or not all(isinstance(value, str) for value in permissions):
            raise ValueError(f"{manifest_path.relative_to(ROOT)}: permissions must be a list of strings")

        package_name = f"{name}.tool"
        package_path = manifest_path.parent / package_name
        if not package_path.is_file():
            raise FileNotFoundError(f"Missing package for {tool_id}: {package_path.relative_to(ROOT)}")

        relative_package = package_path.relative_to(ROOT).as_posix()
        readme_path = manifest_path.parent / "README.md"
        icon_path = manifest_path.parent / ICON_FILENAME
        tool = {
            "iconSource": icon_path if icon_path.is_file() else None,
            "id": tool_id,
            "name": name,
            "description": description,
            "version": version,
            "permissions": permissions,
            "downloadUrl": repository_url(relative_package, raw=True),
            "readmeUrl": repository_url(readme_path.relative_to(ROOT).as_posix())
            if readme_path.is_file()
            else None,
        }
        tools.append(tool)

    return sorted(tools, key=lambda tool: str(tool["name"]).casefold())


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate the KTRX tools catalog site.")
    parser.add_argument("--output", type=Path, default=ROOT / "_site")
    args = parser.parse_args()

    tools = load_tools()
    if not tools:
        raise ValueError("No packaged tools were found.")

    output_dir = args.output.resolve()
    if output_dir == ROOT or output_dir == SITE_DIR.resolve():
        raise ValueError("The output directory must not overwrite the repository or site sources.")
    output_dir.mkdir(parents=True, exist_ok=True)
    for source in SITE_DIR.iterdir():
        destination = output_dir / source.name
        if source.is_dir():
            shutil.copytree(source, destination, dirs_exist_ok=True)
        else:
            shutil.copy2(source, destination)

    icons_dir = output_dir / ICONS_OUTPUT_DIR
    published_tools = []
    for tool in tools:
        icon_source = tool.pop("iconSource")
        if icon_source is not None:
            icons_dir.mkdir(parents=True, exist_ok=True)
            icon_name = f"{str(tool['name']).removeprefix('K~')}.png"
            shutil.copy2(icon_source, icons_dir / icon_name)
            tool["iconUrl"] = f"{ICONS_OUTPUT_DIR}/{icon_name}"
        else:
            tool["iconUrl"] = None
            print(f"Warning: {tool['name']} has no {ICON_FILENAME}; its card will have no visual.")
        published_tools.append(tool)

    (output_dir / "catalog.json").write_text(
        json.dumps({"tools": published_tools}, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"Generated catalog for {len(tools)} tools in {output_dir}")


if __name__ == "__main__":
    main()
