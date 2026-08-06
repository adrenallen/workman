#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
Install Workman from the stable channel (default) or latest channel.

Usage:
  curl -fsSL https://workman.userdefined.io/install.sh | \
    sh -s -- --key <download-key> [--channel stable|latest] [--yes]

  curl -fsSL https://workman.userdefined.io/install.sh | \
    WORKMAN_KEY=<download-key> WORKMAN_CHANNEL=latest sh

Options:
  --key <download-key>  Shared Workman download key (overrides WORKMAN_KEY)
  --channel <channel>   Release channel: stable (default) or latest
  --yes                 Replace superseded launchers without prompting
  --help, -h            Show this help

Environment:
  WORKMAN_KEY          Shared Workman download key
  WORKMAN_CHANNEL      Release channel: stable (default) or latest
  WORKMAN_INSTALL_DIR  Versioned bundle destination
EOF
}

download_key="${WORKMAN_KEY:-}"
channel="${WORKMAN_CHANNEL:-stable}"
assume_yes=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --key)
      if [ "$#" -lt 2 ]; then
        echo "--key requires a value" >&2
        usage >&2
        exit 2
      fi
      download_key="$2"
      shift 2
      ;;
    --key=*)
      download_key="${1#--key=}"
      shift
      ;;
    --channel)
      if [ "$#" -lt 2 ]; then
        echo "--channel requires a value" >&2
        usage >&2
        exit 2
      fi
      channel="$2"
      shift 2
      ;;
    --channel=*)
      channel="${1#--channel=}"
      shift
      ;;
    --yes|-y)
      assume_yes=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

case "$channel" in
  stable|latest) ;;
  *)
    echo "invalid Workman channel: $channel (expected stable or latest)" >&2
    usage >&2
    exit 2
    ;;
esac

if [ -z "$download_key" ]; then
  echo "a Workman download key is required; pass --key or set WORKMAN_KEY" >&2
  usage >&2
  exit 2
fi

for command in curl python3 bash ps; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "required command not found: $command" >&2
    exit 1
  fi
done

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target="macos-arm64"; archive_kind="zip" ;;
  Linux-x86_64) target="linux-x86_64"; archive_kind="tar" ;;
  Linux-aarch64|Linux-arm64) target="linux-arm64"; archive_kind="tar" ;;
  *)
    echo "Workman does not publish a bundle for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

base_url="${WORKMAN_UPDATE_BASE_URL:-https://workman.userdefined.io}"
temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/workman-install.XXXXXX")"
trap 'rm -rf "$temporary_dir"' 0
manifest_path="$temporary_dir/releases.json"
release_metadata_path="$temporary_dir/release-metadata"
archive_path="$temporary_dir/release"
stage_dir="$temporary_dir/stage"
inventory_path="$temporary_dir/install-inventory.json"
reconciler_path="$temporary_dir/reconcile-installs.py"
mkdir -p "$stage_dir"

fetch_with_key() {
  curl --fail --silent --show-error --location --retry 3 \
    --header "Authorization: Bearer $download_key" "$@"
}

echo "Reading the Workman $channel channel..."
fetch_with_key "$base_url/releases.json" --output "$manifest_path"

python3 - "$manifest_path" "$target" "$base_url" "$channel" > "$release_metadata_path" <<'PY'
import json
import re
import sys
from urllib.parse import urlparse

manifest_path, target, base_url, channel = sys.argv[1:]
with open(manifest_path, encoding="utf-8") as source:
    manifest = json.load(source)

release = manifest["channels"][channel]
version = release["version"]
if re.fullmatch(r"\d+\.\d+\.\d+", version) is None:
    raise SystemExit(f"the update server returned an invalid {channel} version")

asset = next((candidate for candidate in release["assets"] if candidate["target"] == target), None)
if asset is None:
    raise SystemExit(f"the stable release has no {target} bundle")

sha256 = asset["sha256"]
if re.fullmatch(r"[a-f0-9]{64}", sha256) is None:
    raise SystemExit("the update server returned an invalid artifact checksum")

artifact_url = urlparse(asset["url"])
server_url = urlparse(base_url)
if (
    artifact_url.scheme != server_url.scheme
    or artifact_url.netloc != server_url.netloc
    or not artifact_url.path.startswith(f"/versions/{version}/")
):
    raise SystemExit("the update server returned an untrusted artifact URL")

print(version)
print(asset["url"])
print(sha256)
PY

version=
artifact_url=
expected_sha256=
{
  IFS= read -r version || :
  IFS= read -r artifact_url || :
  IFS= read -r expected_sha256 || :
} < "$release_metadata_path"

if [ -z "$version" ] || [ -z "$artifact_url" ] || [ -z "$expected_sha256" ]; then
  echo "the update server returned incomplete release metadata" >&2
  exit 1
fi

printf 'Selected Workman %s from the %s channel.\n' "$version" "$channel"
echo "Downloading Workman $version for $target..."
fetch_with_key "$artifact_url" --output "$archive_path"

if command -v sha256sum >/dev/null 2>&1; then
  actual_sha256="$(sha256sum "$archive_path" | awk '{print $1}')"
else
  actual_sha256="$(shasum -a 256 "$archive_path" | awk '{print $1}')"
fi
if [ "$actual_sha256" != "$expected_sha256" ]; then
  echo "download checksum mismatch: expected $expected_sha256, got $actual_sha256" >&2
  exit 1
fi

case "$archive_kind" in
  zip)
    command -v unzip >/dev/null 2>&1 || { echo "required command not found: unzip" >&2; exit 1; }
    unzip -q "$archive_path" -d "$stage_dir"
    ;;
  tar)
    tar -xzf "$archive_path" -C "$stage_dir"
    ;;
esac

install_dir="${WORKMAN_INSTALL_DIR:-$HOME/.local/share/workman/$version}"
if [ ! -f "$stage_dir/install.sh" ] || [ ! -x "$stage_dir/bin/wrk" ] || [ ! -x "$stage_dir/bin/workmand" ]; then
  echo "the Workman bundle is missing its installer or executable pair" >&2
  exit 1
fi

cat > "$reconciler_path" <<'PY'
import glob
import json
import os
import plistlib
import re
import shlex
import shutil
import signal
import subprocess
import sys
import time

PROGRAM_TARGET = {
    "wrk": "wrk",
    "awm": "wrk",
    "workmand": "workmand",
    "awmd": "workmand",
}


def absolute(path):
    return os.path.abspath(os.path.expanduser(path))


def exists(path):
    return os.path.lexists(path)


def real(path):
    return os.path.realpath(path)


def version_for(path, program):
    if PROGRAM_TARGET[program] != "wrk" or not os.access(path, os.X_OK):
        return None
    try:
        completed = subprocess.run(
            [path, "--version"],
            check=False,
            capture_output=True,
            text=True,
            timeout=3,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    output = (completed.stdout or completed.stderr).strip().splitlines()
    return output[0] if output else None


def add_launcher(launchers, path, program, source):
    path = absolute(path)
    if not exists(path):
        return
    key = (path, program)
    record = launchers.get(key)
    if record is None:
        record = {
            "path": path,
            "program": program,
            "sources": [],
            "real_path": real(path),
            "version": version_for(path, program),
        }
        launchers[key] = record
    if source not in record["sources"]:
        record["sources"].append(source)


def scan_launchers(home, path_value):
    launchers = {}
    programs = tuple(PROGRAM_TARGET)
    which = shutil_which("which", path_value)
    if which:
        for program in programs:
            completed = subprocess.run(
                [which, "-a", program],
                check=False,
                capture_output=True,
                text=True,
                env={**os.environ, "PATH": path_value},
            )
            for line in completed.stdout.splitlines():
                add_launcher(launchers, line.strip(), program, "which -a")

    seen_directories = set()
    for entry in path_value.split(os.pathsep):
        directory = absolute(entry or ".")
        if directory in seen_directories:
            continue
        seen_directories.add(directory)
        for program in programs:
            add_launcher(launchers, os.path.join(directory, program), program, "PATH")

    test_root = os.environ.get("WORKMAN_INSTALL_TEST_ROOT")
    system_directories = (
        (
            os.path.join(test_root, "usr", "local", "bin"),
            os.path.join(test_root, "opt", "homebrew", "bin"),
        )
        if test_root
        else ("/usr/local/bin", "/opt/homebrew/bin")
    )
    for directory in (os.path.join(home, ".local", "bin"), *system_directories):
        for program in programs:
            add_launcher(launchers, os.path.join(directory, program), program, "known launcher")
    return sorted(launchers.values(), key=lambda item: (item["path"], item["program"]))


def shutil_which(program, path_value):
    for entry in path_value.split(os.pathsep):
        candidate = os.path.join(entry or ".", program)
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return absolute(candidate)
    return None


def scan_historical(home):
    patterns = (
        (os.path.join(home, ".local", "share", "workman", "*", "bin", "wrk"), "wrk"),
        (os.path.join(home, ".local", "share", "workman", "*", "bin", "workmand"), "workmand"),
        (os.path.join(home, ".local", "share", "awm", "*", "bin", "awm"), "awm"),
        (os.path.join(home, ".local", "share", "awm", "*", "bin", "awmd"), "awmd"),
        (os.path.join(home, ".local", "share", "awm", "*", "awm"), "awm"),
        (os.path.join(home, ".local", "share", "awm", "*", "awmd"), "awmd"),
    )
    found = {}
    for pattern, program in patterns:
        for path in glob.glob(pattern):
            path = absolute(path)
            key = (path, program)
            if key not in found:
                found[key] = {
                    "path": path,
                    "program": program,
                    "real_path": real(path),
                    "version": version_for(path, program),
                }
    return sorted(found.values(), key=lambda item: (item["path"], item["program"]))


def daemon_from_args(pid, args, raw, exact_args):
    executable_index = None
    for index, value in enumerate(args):
        if os.path.basename(value) in ("workmand", "awmd"):
            executable_index = index
            break
    if executable_index is None:
        match = re.search(r"(?:^|\s)(\S*(?:workmand|awmd))(?=\s|$)", raw)
        if match is None:
            return None
        executable = match.group(1)
    else:
        executable = args[executable_index]

    data_dir = None
    port = None
    if exact_args and "--data-dir" in args:
        index = args.index("--data-dir")
        if index + 1 < len(args):
            data_dir = args[index + 1]
    elif " --data-dir " in raw:
        data_dir = raw.split(" --data-dir ", 1)[1]
        if " --port " in data_dir:
            data_dir, port = data_dir.rsplit(" --port ", 1)
    if "--port" in args:
        index = args.index("--port")
        if index + 1 < len(args):
            port = args[index + 1]

    executable = absolute(executable)
    sibling = os.path.join(
        os.path.dirname(executable),
        "awm" if os.path.basename(executable) == "awmd" else "wrk",
    )
    return {
        "pid": pid,
        "path": executable,
        "real_path": real(executable),
        "version": version_for(sibling, os.path.basename(sibling)) if exists(sibling) else None,
        "data_dir": data_dir,
        "port": port,
        "command": raw,
    }


def scan_daemons():
    daemons = {}
    if sys.platform.startswith("linux") and os.path.isdir("/proc"):
        for entry in os.listdir("/proc"):
            if not entry.isdigit():
                continue
            pid = int(entry)
            try:
                raw_bytes = open(f"/proc/{pid}/cmdline", "rb").read()
                args = [item.decode(errors="replace") for item in raw_bytes.split(b"\0") if item]
            except (OSError, PermissionError):
                continue
            raw = " ".join(shlex.quote(item) for item in args)
            record = daemon_from_args(pid, args, raw, True)
            if record:
                daemons[pid] = record
    else:
        completed = subprocess.run(
            ["ps", "-axo", "pid=,command="],
            check=True,
            capture_output=True,
            text=True,
        )
        for line in completed.stdout.splitlines():
            match = re.match(r"\s*(\d+)\s+(.*)", line)
            if match is None:
                continue
            pid = int(match.group(1))
            raw = match.group(2)
            try:
                args = shlex.split(raw)
            except ValueError:
                args = raw.split()
            record = daemon_from_args(pid, args, raw, False)
            if record:
                daemons[pid] = record
    records = [daemons[pid] for pid in sorted(daemons)]
    test_root = os.environ.get("WORKMAN_INSTALL_TEST_ROOT")
    if test_root:
        test_root = absolute(test_root)
        records = [
            record
            for record in records
            if any(
                value and (absolute(value) == test_root or absolute(value).startswith(test_root + os.sep))
                for value in (record.get("path"), record.get("real_path"), record.get("data_dir"))
            )
        ]
    return records


def scan(home, path_value, install_dir, expected_version, inventory_path):
    home = absolute(home)
    install_dir = absolute(install_dir)
    inventory = {
        "home": home,
        "path": path_value,
        "install_dir": install_dir,
        "managed_bin_dir": os.path.join(home, ".local", "bin"),
        "new_wrk": os.path.join(install_dir, "bin", "wrk"),
        "new_workmand": os.path.join(install_dir, "bin", "workmand"),
        "expected_version": expected_version,
        "launchers": scan_launchers(home, path_value),
        "historical": scan_historical(home),
        "daemons": scan_daemons(),
    }
    with open(inventory_path, "w", encoding="utf-8") as output:
        json.dump(inventory, output)


def load(path):
    with open(path, encoding="utf-8") as source:
        return json.load(source)


def label(record):
    version = f" ({record['version']})" if record.get("version") else ""
    target = record.get("real_path")
    arrow = f" -> {target}" if target and target != record["path"] else ""
    return f"{record['program']}{version}: {record['path']}{arrow}"


def report(inventory):
    launchers = inventory["launchers"]
    historical = inventory["historical"]
    daemons = inventory["daemons"]
    if launchers:
        print("Existing Workman launchers (deduplicated):")
        for record in launchers:
            print(f"  {label(record)}")
    else:
        print("No existing Workman launchers found.")
    if historical:
        print("Historical versioned bundles (kept as rollback files):")
        for record in historical:
            print(f"  {label(record)}")
    if daemons:
        print("Running Workman daemons:")
        for record in daemons:
            version = f"; {record['version']}" if record.get("version") else ""
            print(f"  pid {record['pid']}: {record['path']}{version}")


def action_count(inventory):
    targets = {
        program: real(inventory[f"new_{target}"])
        for program, target in PROGRAM_TARGET.items()
    }
    launchers = sum(
        real(record["path"]) != targets[record["program"]]
        for record in inventory["launchers"]
    )
    daemons = sum(
        record["real_path"] != real(inventory["new_workmand"])
        or record.get("version") != f"workman {inventory['expected_version']}"
        for record in inventory["daemons"]
    )
    return launchers, daemons


def next_backup(path):
    stamp = time.strftime("%Y%m%d%H%M%S", time.gmtime())
    candidate = f"{path}.workman-backup-{stamp}"
    suffix = 1
    while exists(candidate):
        candidate = f"{path}.workman-backup-{stamp}-{suffix}"
        suffix += 1
    return candidate


def replace_launcher(path, target):
    if real(path) == real(target):
        return None
    parent = os.path.dirname(path)
    if not os.access(parent, os.W_OK):
        raise PermissionError(
            f"cannot replace {path}: {parent} is not writable; remove it manually or rerun with suitable permissions"
        )
    backup = next_backup(path)
    temporary = os.path.join(parent, f".workman-link-{os.getpid()}-{os.path.basename(path)}")
    os.rename(path, backup)
    try:
        os.symlink(target, temporary)
        os.replace(temporary, path)
    except BaseException:
        if exists(temporary):
            os.unlink(temporary)
        os.rename(backup, path)
        raise
    return backup


def apply(inventory):
    failures = []
    needs_privilege = []
    for record in inventory["launchers"]:
        target = inventory[f"new_{PROGRAM_TARGET[record['program']]}"]
        if not exists(record["path"]):
            continue
        try:
            backup = replace_launcher(record["path"], target)
            if backup:
                print(f"Replaced {record['path']} -> {target}")
                print(f"  Backup: {backup}")
        except PermissionError as error:
            needs_privilege.append(str(error))
        except OSError as error:
            failures.append(str(error))
    if failures:
        for failure in failures:
            print(f"ERROR: {failure}", file=sys.stderr)
        raise SystemExit(1)
    if needs_privilege:
        for failure in needs_privilege:
            print(f"Administrator permission required: {failure}", file=sys.stderr)
        raise SystemExit(77)


def process_alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False
    except PermissionError:
        return True


def restart_daemons(inventory):
    target = inventory["new_workmand"]
    failures = []
    for record in inventory["daemons"]:
        if (
            record["real_path"] == real(target)
            and record.get("version") == f"workman {inventory['expected_version']}"
        ) or not process_alive(record["pid"]):
            continue
        if not record.get("data_dir"):
            failures.append(
                f"cannot safely restart pid {record['pid']}; its --data-dir could not be recovered: {record['command']}"
            )
            continue
        print(
            f"Restarting daemon pid {record['pid']} with {target}; preserving data dir {record['data_dir']}"
        )
        try:
            os.kill(record["pid"], signal.SIGTERM)
        except (ProcessLookupError, PermissionError) as error:
            failures.append(f"could not stop daemon pid {record['pid']}: {error}")
            continue
        deadline = time.monotonic() + 8
        while process_alive(record["pid"]) and time.monotonic() < deadline:
            time.sleep(0.1)
        if process_alive(record["pid"]):
            failures.append(f"daemon pid {record['pid']} did not stop after SIGTERM")
            continue
        arguments = [target, "--data-dir", record["data_dir"]]
        if record.get("port"):
            arguments.extend(["--port", record["port"]])
        try:
            process = subprocess.Popen(
                arguments,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
        except OSError as error:
            failures.append(f"could not start replacement daemon for pid {record['pid']}: {error}")
            continue
        time.sleep(0.5)
        if process.poll() is not None:
            failures.append(
                f"replacement daemon for pid {record['pid']} exited immediately with {process.returncode}"
            )
        else:
            print(f"Restarted daemon as pid {process.pid}.")
    if failures:
        for failure in failures:
            print(f"ERROR: {failure}", file=sys.stderr)
        raise SystemExit(1)


def fresh_resolve(path_value, program):
    seen = set()
    for entry in path_value.split(os.pathsep):
        directory = absolute(entry or ".")
        if directory in seen:
            continue
        seen.add(directory)
        candidate = os.path.join(directory, program)
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return absolute(candidate)
    return None


def verify(inventory, expected_version):
    resolved = fresh_resolve(inventory["path"], "wrk")
    expected = inventory["new_wrk"]
    managed = os.path.join(inventory["managed_bin_dir"], "wrk")
    if resolved is None:
        print(
            f"ERROR: a fresh PATH walk cannot find wrk. The new launcher is {managed}; add its directory to PATH.",
            file=sys.stderr,
        )
        raise SystemExit(1)
    if real(resolved) != real(expected):
        print(
            f"ERROR: a fresh PATH walk still selects {resolved} -> {real(resolved)}, not the just-installed {expected}.",
            file=sys.stderr,
        )
        print(
            f"Remove or replace the offending launcher {resolved}, or put {inventory['managed_bin_dir']} earlier in PATH.",
            file=sys.stderr,
        )
        raise SystemExit(1)
    completed = subprocess.run(
        [resolved, "--version"],
        check=False,
        capture_output=True,
        text=True,
        timeout=5,
    )
    output = (completed.stdout or completed.stderr).strip()
    expected_output = f"workman {expected_version}"
    if completed.returncode != 0 or output != expected_output:
        print(
            f"ERROR: {resolved} resolved to the new binary but reported {output!r}; expected {expected_output!r}.",
            file=sys.stderr,
        )
        raise SystemExit(1)
    if absolute(resolved) != absolute(managed):
        print(
            f"Note: fresh PATH resolution uses {resolved}, not {managed}; it now points to the just-installed binary."
        )
    print(f"Verified fresh PATH resolution: {resolved} reports {expected_output}.")


def app_bundle_id(bundle):
    plist = os.path.join(bundle, "Contents", "Info.plist")
    try:
        with open(plist, "rb") as source:
            return plistlib.load(source).get("CFBundleIdentifier")
    except (OSError, plistlib.InvalidFileException):
        return None


def install_app(source, destination, expected_identifier):
    source = absolute(source)
    destination = absolute(destination)
    identifier = app_bundle_id(source)
    if identifier != expected_identifier:
        raise SystemExit(
            f"refusing to install {source}: bundle id is {identifier!r}, expected {expected_identifier!r}"
        )
    if exists(destination):
        installed_identifier = app_bundle_id(destination)
        if installed_identifier != expected_identifier:
            raise SystemExit(
                f"refusing to replace {destination}: bundle id is {installed_identifier!r}, not {expected_identifier!r}"
            )

    parent = os.path.dirname(destination)
    os.makedirs(parent, exist_ok=True)
    temporary = os.path.join(parent, f".Workman.app.install-{os.getpid()}")
    backup = os.path.join(parent, f".Workman.app.replace-{os.getpid()}")
    if exists(temporary) or exists(backup):
        raise SystemExit("temporary Workman.app replacement path already exists; retry the install")
    try:
        subprocess.run(["ditto", source, temporary], check=True)
        copied_identifier = app_bundle_id(temporary)
        if copied_identifier != expected_identifier:
            raise RuntimeError(
                f"copied app bundle id is {copied_identifier!r}, expected {expected_identifier!r}"
            )
        if exists(destination):
            os.rename(destination, backup)
        try:
            os.rename(temporary, destination)
        except BaseException:
            if exists(backup):
                os.rename(backup, destination)
            raise
        if exists(backup):
            if os.path.isdir(backup) and not os.path.islink(backup):
                shutil.rmtree(backup)
            else:
                os.unlink(backup)
    finally:
        if exists(temporary):
            if os.path.isdir(temporary) and not os.path.islink(temporary):
                shutil.rmtree(temporary)
            else:
                os.unlink(temporary)


def main():
    command = sys.argv[1]
    if command == "scan":
        scan(*sys.argv[2:])
        return
    if command == "app-bundle-id":
        identifier = app_bundle_id(sys.argv[2])
        if identifier:
            print(identifier)
        return
    if command == "install-app":
        install_app(*sys.argv[2:])
        return
    inventory = load(sys.argv[2])
    if command == "report":
        report(inventory)
    elif command == "counts":
        print(*action_count(inventory))
    elif command == "apply":
        apply(inventory)
    elif command == "restart":
        restart_daemons(inventory)
    elif command == "verify":
        verify(inventory, sys.argv[3])
    else:
        raise SystemExit(f"unknown reconciler command: {command}")


if __name__ == "__main__":
    main()
PY

has_controlling_tty() {
  (tty </dev/tty) >/dev/null 2>&1
}

python3 "$reconciler_path" scan "$HOME" "${PATH:-}" "$install_dir" "$version" "$inventory_path"
python3 "$reconciler_path" report "$inventory_path"
set -- $(python3 "$reconciler_path" counts "$inventory_path")
launcher_actions="$1"
daemon_actions="$2"

restart_daemons=0
if [ "$launcher_actions" -gt 0 ] || [ "$daemon_actions" -gt 0 ]; then
  proceed=1
  if [ "$assume_yes" -eq 0 ] && has_controlling_tty; then
    printf '\nReplace %s superseded launcher(s) with Workman %s? [Y/n] ' \
      "$launcher_actions" "$version" > /dev/tty
    answer=
    IFS= read -r answer < /dev/tty || :
    case "$answer" in
      n|N|no|NO) proceed=0 ;;
    esac
  elif [ "$assume_yes" -eq 0 ]; then
    echo "No interactive terminal; proceeding with replacement (same behavior as --yes)."
  fi
  if [ "$proceed" -ne 1 ]; then
    echo "Install cancelled before changing existing launchers." >&2
    exit 1
  fi
  if [ "$daemon_actions" -gt 0 ]; then
    restart_daemons=1
    if [ "$assume_yes" -eq 0 ] && has_controlling_tty; then
      printf 'Restart %s running Workman daemon(s) with the new binary, preserving data dirs? [Y/n] ' \
        "$daemon_actions" > /dev/tty
      answer=
      IFS= read -r answer < /dev/tty || :
      case "$answer" in
        n|N|no|NO) restart_daemons=0 ;;
      esac
    fi
  fi
fi

mkdir -p "$install_dir"
cp -R "$stage_dir/." "$install_dir/"
if [ ! -f "$install_dir/install.sh" ]; then
  echo "the Workman bundle does not contain install.sh" >&2
  exit 1
fi

bash "$install_dir/install.sh" </dev/null
apply_status=0
python3 "$reconciler_path" apply "$inventory_path" || apply_status="$?"
if [ "$apply_status" -eq 77 ]; then
  if ! command -v sudo >/dev/null 2>&1; then
    echo "administrator permission is required to replace a protected launcher, but sudo is unavailable" >&2
    exit 1
  fi
  echo "Administrator permission is needed to replace protected Workman launchers."
  sudo python3 "$reconciler_path" apply "$inventory_path"
elif [ "$apply_status" -ne 0 ]; then
  exit "$apply_status"
fi
if [ "$restart_daemons" -eq 1 ]; then
  python3 "$reconciler_path" restart "$inventory_path"
elif [ "$daemon_actions" -gt 0 ]; then
  echo "Warning: an older Workman daemon is still running; restart it before relying on daemon changes." >&2
fi

verify_ok=1
python3 "$reconciler_path" verify "$inventory_path" "$version" || verify_ok=0
printf 'If your current shell cached an older command path, run: hash -r\n'
if [ "$verify_ok" -ne 1 ]; then
  exit 1
fi

installed_app=
if [ "$(uname -s)" = Darwin ] && [ -d "$install_dir/Workman.app" ]; then
  expected_app_identifier=com.workman.desktop
  source_app_identifier="$(python3 "$reconciler_path" app-bundle-id "$install_dir/Workman.app")"
  if [ "$source_app_identifier" != "$expected_app_identifier" ]; then
    echo "refusing to install Workman.app: bundle id is '$source_app_identifier', expected '$expected_app_identifier'" >&2
    exit 1
  fi
  if [ -n "${WORKMAN_INSTALL_TEST_ROOT:-}" ]; then
    applications_dir="$WORKMAN_INSTALL_TEST_ROOT/Applications"
  else
    applications_dir=/Applications
  fi
  destination_app="$applications_dir/Workman.app"
  install_desktop_app=1
  if [ "$assume_yes" -eq 0 ] && has_controlling_tty; then
    printf '\nCopy Workman.app to %s for Launchpad and Spotlight? [Y/n] ' \
      "$destination_app" > /dev/tty
    answer=
    IFS= read -r answer < /dev/tty || :
    case "$answer" in
      n|N|no|NO) install_desktop_app=0 ;;
    esac
  elif [ "$assume_yes" -eq 0 ]; then
    echo "No interactive terminal; installing Workman.app in $applications_dir (same behavior as --yes)."
  fi
  if [ "$install_desktop_app" -eq 1 ]; then
    if ! command -v ditto >/dev/null 2>&1; then
      echo "required command not found for Workman.app installation: ditto" >&2
      exit 1
    fi
    if [ -w "$applications_dir" ] || { [ ! -e "$applications_dir" ] && [ -w "$(dirname "$applications_dir")" ]; }; then
      python3 "$reconciler_path" install-app \
        "$install_dir/Workman.app" "$destination_app" "$expected_app_identifier"
    elif command -v sudo >/dev/null 2>&1; then
      echo "Administrator permission is needed to refresh $destination_app."
      sudo python3 "$reconciler_path" install-app \
        "$install_dir/Workman.app" "$destination_app" "$expected_app_identifier"
    else
      echo "cannot write to $applications_dir and sudo is unavailable" >&2
      exit 1
    fi
    installed_app="$destination_app"
    echo "Installed $destination_app (available in Launchpad and Spotlight)."
  else
    echo "Desktop app left at $install_dir/Workman.app."
  fi
fi
printf '\nInstalled Workman %s from a checksum-verified bundle at %s.\n' "$version" "$install_dir"
