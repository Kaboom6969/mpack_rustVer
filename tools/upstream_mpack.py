#!/usr/bin/env python3
"""Resolve the pinned upstream MPack checkout used by fuzz and benchmarks."""

from __future__ import annotations

import argparse
import json
import os
import stat
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError as exc:  # pragma: no cover - Python 3.11+ in practice
    raise SystemExit("Python 3.11+ with tomllib is required") from exc


ROOT = Path(__file__).resolve().parents[1]
PORT_MORTEM = ROOT / ".port-mortem.toml"
PINNED_DIR = ROOT / "target" / "upstream" / "mpack" / "pinned"
METADATA_FILE = PINNED_DIR / ".resolved.json"


def load_config() -> tuple[str, str | None, str | None]:
    data = tomllib.loads(PORT_MORTEM.read_text(encoding="utf-8"))
    source_url = data.get("source_url")
    source_version = data.get("source_version")
    kickoff_hash = data.get("kickoff_hash")
    if not source_url:
        raise SystemExit(f"Missing source_url in {PORT_MORTEM}")
    return (
        str(source_url),
        str(source_version) if source_version else None,
        str(kickoff_hash) if kickoff_hash else None,
    )


def src_dir() -> Path:
    return PINNED_DIR / "src"


def git(*args: str, cwd: Path | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["git", *args],
        cwd=cwd or ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )


def current_head(repo_dir: Path) -> str | None:
    if not repo_dir.exists():
        return None
    try:
        return git("-C", str(repo_dir), "rev-parse", "HEAD").stdout.strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None


def read_metadata() -> dict[str, str] | None:
    if not METADATA_FILE.exists():
        return None
    return json.loads(METADATA_FILE.read_text(encoding="utf-8"))


def expected_metadata(
    source_url: str,
    source_version: str | None,
    kickoff_hash: str | None,
    *,
    resolution_kind: str | None = None,
    resolved_commit: str | None = None,
) -> dict[str, str]:
    metadata = {
        "source_url": source_url,
        "source_version": source_version or "",
        "kickoff_hash": kickoff_hash or "",
    }
    if resolution_kind is not None:
        metadata["resolution_kind"] = resolution_kind
    if resolved_commit is not None:
        metadata["resolved_commit"] = resolved_commit
    return metadata


def metadata_matches_pin(
    metadata: dict[str, str] | None,
    source_url: str,
    source_version: str | None,
    kickoff_hash: str | None,
) -> bool:
    if not metadata:
        return False
    expected = expected_metadata(source_url, source_version, kickoff_hash)
    return all(metadata.get(key, "") == value for key, value in expected.items())


def checkout_ref(repo_dir: Path, ref: str) -> bool:
    try:
        git(
            "-C",
            str(repo_dir),
            "-c",
            "advice.detachedHead=false",
            "checkout",
            ref,
        )
        return True
    except subprocess.CalledProcessError:
        return False


def resolve_checkout(repo_dir: Path, source_version: str | None, kickoff_hash: str | None) -> tuple[str, str]:
    attempts: list[tuple[str, str]] = []
    if kickoff_hash:
        attempts.append(("kickoff_hash", kickoff_hash))
    if source_version:
        attempts.append(("source_version", f"v{source_version}"))
        attempts.append(("source_version", source_version))

    attempted_refs: list[str] = []
    for resolution_kind, ref in attempts:
        attempted_refs.append(ref)
        if checkout_ref(repo_dir, ref):
            resolved_commit = current_head(repo_dir)
            if not resolved_commit:
                raise SystemExit(f"Checked out {ref} but could not resolve HEAD")
            return resolution_kind, resolved_commit

    joined = ", ".join(attempted_refs) if attempted_refs else "<none>"
    raise SystemExit(
        "Failed to resolve pinned upstream MPack checkout from "
        f"{PORT_MORTEM}; tried refs: {joined}"
    )


def ensure_checkout() -> Path:
    if shutil.which("git") is None:
        raise SystemExit("git is required to fetch the pinned upstream MPack checkout")

    source_url, source_version, kickoff_hash = load_config()
    PINNED_DIR.parent.mkdir(parents=True, exist_ok=True)

    metadata = read_metadata()
    if (
        metadata
        and PINNED_DIR.exists()
        and metadata_matches_pin(metadata, source_url, source_version, kickoff_hash)
        and current_head(PINNED_DIR) == metadata.get("resolved_commit")
    ):
        return PINNED_DIR

    if PINNED_DIR.exists():
        shutil.rmtree(PINNED_DIR)

    temp_parent = PINNED_DIR.parent
    temp_dir = Path(
        tempfile.mkdtemp(
            prefix="mpack-upstream.",
            suffix=".tmp",
            dir=temp_parent,
        )
    )
    try:
        git("clone", source_url, str(temp_dir))
        resolution_kind, resolved_commit = resolve_checkout(
            temp_dir, source_version, kickoff_hash
        )
        (temp_dir / ".resolved.json").write_text(
            json.dumps(
                expected_metadata(
                    source_url,
                    source_version,
                    kickoff_hash,
                    resolution_kind=resolution_kind,
                    resolved_commit=resolved_commit,
                ),
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        temp_dir.replace(PINNED_DIR)
        return PINNED_DIR
    except FileNotFoundError as exc:
        raise SystemExit("git is required to fetch the pinned upstream MPack checkout") from exc
    except subprocess.CalledProcessError as exc:
        detail = exc.stderr.strip() or exc.stdout.strip() or str(exc)
        raise SystemExit(f"Failed to fetch pinned upstream MPack checkout: {detail}") from exc
    finally:
        if temp_dir.exists():
            shutil.rmtree(temp_dir, ignore_errors=True)


def cleanup_checkout() -> int:
    if not PINNED_DIR.exists():
        return 0
    remove_tree(PINNED_DIR)
    return 0


def handle_remove_readonly(function, path: str, excinfo) -> None:
    os.chmod(path, stat.S_IWRITE)
    function(path)


def remove_tree(path: Path) -> None:
    kwargs = {"ignore_errors": False}
    if sys.version_info >= (3, 12):
        kwargs["onexc"] = handle_remove_readonly
    else:
        kwargs["onerror"] = handle_remove_readonly
    shutil.rmtree(path, **kwargs)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    subparsers.add_parser("ensure", help="Ensure the pinned upstream checkout exists")
    subparsers.add_parser("path", help="Print the pinned upstream src path")
    subparsers.add_parser("cleanup", help="Delete the pinned upstream checkout cache")
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    if args.command == "ensure":
        ensure_checkout()
        print(src_dir())
        return 0
    if args.command == "path":
        print(src_dir())
        return 0
    if args.command == "cleanup":
        return cleanup_checkout()
    parser.error(f"unknown command: {args.command}")
    return 2


if __name__ == "__main__":
    sys.exit(main())
