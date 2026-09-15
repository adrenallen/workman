#!/usr/bin/env python3
"""Exercise installer completion in disposable directories, without installing or opening Workman."""

import errno
import json
import os
from pathlib import Path
import pty
import select
import subprocess
import tempfile
import time
import unittest


REPO = Path(__file__).resolve().parents[2]
HELPER = REPO / "scripts/lib/dev-install-cleanup.sh"
RELAUNCH_HELPER = REPO / "scripts/lib/dev-install-relaunch.sh"


class CompletionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="workman-cleanup-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "source checkout"
        self.build = self.repo / "target"
        self.dist = self.repo / "apps/desktop/dist"
        self.installed = [self.root / name for name in ["installed binaries", "launchers", "Workman Dev.app", "application data"]]
        for directory in [self.build, self.dist, *self.installed]:
            directory.mkdir(parents=True)
            (directory / "keep-or-clean").write_text("fixture")
        (self.repo / "Cargo.toml").write_text("source stays")
        self.bin = self.root / "mock-bin"
        self.bin.mkdir()
        self.log = self.root / "cargo.json"
        cargo = self.bin / "cargo"
        cargo.write_text("""#!/usr/bin/env python3
import json, os, shutil, sys
from pathlib import Path
args = sys.argv[1:]
Path(os.environ['CLEANUP_TEST_LOG']).write_text(json.dumps(args))
if os.environ.get('CLEANUP_TEST_FAIL') == '1': sys.exit(7)
assert args[0] == 'clean'
target = Path(args[args.index('--target-dir') + 1]).resolve()
assert Path(os.environ['CLEANUP_TEST_ROOT']).resolve() in target.parents, 'Mock cleanup must stay inside the fixture'
shutil.rmtree(target)
""")
        cargo.chmod(0o755)
        self.env = {**os.environ, "PATH": f"{self.bin}:{os.environ['PATH']}", "CLEANUP_TEST_LOG": str(self.log), "CLEANUP_TEST_ROOT": str(self.root)}
        self.env.pop("WORKMAN_DEV_RELAUNCH", None)
        self.launch_log = self.root / "launch.log"
        self.env["RELAUNCH_TEST_LOG"] = str(self.launch_log)
        launcher = self.installed[0] / "wrk-dev"
        launcher.write_text('#!/usr/bin/env bash\nprintf "%s\\n" "$@" > "$RELAUNCH_TEST_LOG"\nexit "${RELAUNCH_TEST_EXIT:-0}"\n')
        launcher.chmod(0o755)

    def command(self, mode="ask"):
        return ["bash", "-c", 'set -euo pipefail; source "$1"; shift; workman_dev_cleanup "$@"',
                "cleanup-test", str(HELPER), mode, str(self.repo), str(self.build), *map(str, self.installed)]

    def run_cleanup(self, mode="ask", answer=None):
        return self.run_command(self.command(mode), answer=answer)

    def run_relaunch(self, mode="ask", answer=None):
        command = ["bash", "-c", 'set -euo pipefail; source "$1"; workman_dev_relaunch "$2" "$3"',
                   "relaunch-test", str(RELAUNCH_HELPER), mode, str(self.installed[0])]
        return self.run_command(command, answer=answer, prompt=b"[Y/n]")

    def run_installer_completion(self, *flags, answer=None):
        # Run the installer's real option parsing and completion sequence around
        # fixture paths, skipping the build and installation into the user's home.
        installer = (REPO / "scripts/dev-install.sh").read_text()
        options = installer.split("\nrepo_root=", 1)[0]
        completion = installer[installer.index("\ncompletion_status=0\n"):]
        fixture = '''
source "$COMPLETION_TEST_HELPER"
source "$COMPLETION_TEST_RELAUNCH_HELPER"
repo_root="$COMPLETION_TEST_REPO"
build_dir="$repo_root/target"
install_dir="$CLEANUP_TEST_ROOT/installed binaries"
bin_dir="$CLEANUP_TEST_ROOT/launchers"
app_path="$CLEANUP_TEST_ROOT/Workman Dev.app"
data_dir="$CLEANUP_TEST_ROOT/application data"
'''
        self.env.update({"COMPLETION_TEST_HELPER": str(HELPER),
                         "COMPLETION_TEST_RELAUNCH_HELPER": str(RELAUNCH_HELPER),
                         "COMPLETION_TEST_REPO": str(self.repo)})
        return self.run_command(["bash", "-c", options + fixture + completion,
                                 "installer-completion-test", *flags], answer=answer)

    def run_command(self, command, answer=None, prompt=b"[y/N]"):
        if answer is None:
            result = subprocess.run(command, input=b"y\n", stdout=subprocess.PIPE,
                                    stderr=subprocess.STDOUT, env=self.env, timeout=10)
            return result.returncode, result.stdout.decode()
        master, slave = pty.openpty()
        process = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave, env=self.env)
        os.close(slave)
        output = b""
        sent = False
        deadline = time.monotonic() + 10
        try:
            while time.monotonic() < deadline:
                if select.select([master], [], [], 0.1)[0]:
                    try:
                        data = os.read(master, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            break
                        raise
                    if not data:
                        break
                    output += data
                    if not sent and prompt in output:
                        os.write(master, answer)
                        sent = True
                elif process.poll() is not None:
                    break
            process.wait(timeout=1)
            return process.returncode, output.decode()
        finally:
            if process.poll() is None:
                process.kill()
            process.wait()
            os.close(master)

    def assert_installed_and_source_preserved(self):
        self.assertEqual((self.repo / "Cargo.toml").read_text(), "source stays")
        for directory in self.installed:
            self.assertEqual((directory / "keep-or-clean").read_text(), "fixture")

    def assert_default_completion(self, answer=None):
        code, output = self.run_installer_completion(answer=answer)
        self.assertEqual(code, 0, output)
        self.assertFalse(self.build.exists())
        self.assertFalse(self.dist.exists())
        self.assertEqual(self.launch_log.read_text(), "app\n")
        self.assertLess(output.index("Build output removed"), output.index("Opening Workman Dev"))
        self.assertNotIn("[y/N]", output)
        self.assertNotIn("[Y/n]", output)
        self.assert_installed_and_source_preserved()

    def test_installer_noninteractive_default_cleans_then_launches(self):
        self.assert_default_completion()

    def test_installer_interactive_default_cleans_then_launches_without_prompt(self):
        self.assert_default_completion(answer=b"n\n")

    def test_installer_no_cleanup_keeps_output_and_launches(self):
        code, output = self.run_installer_completion("--no-cleanup")
        self.assertEqual(code, 0, output)
        self.assertTrue(self.build.exists())
        self.assertTrue(self.dist.exists())
        self.assertFalse(self.log.exists())
        self.assertEqual(self.launch_log.read_text(), "app\n")

    def test_installer_no_relaunch_cleans_output_and_leaves_app_closed(self):
        code, output = self.run_installer_completion("--no-relaunch")
        self.assertEqual(code, 0, output)
        self.assertFalse(self.build.exists())
        self.assertFalse(self.dist.exists())
        self.assertFalse(self.launch_log.exists())

    def test_installer_both_opt_out_flags_skip_completion_actions(self):
        code, output = self.run_installer_completion("--no-cleanup", "--no-relaunch")
        self.assertEqual(code, 0, output)
        self.assertTrue(self.build.exists())
        self.assertTrue(self.dist.exists())
        self.assertFalse(self.log.exists())
        self.assertFalse(self.launch_log.exists())

    def test_installer_explicit_enable_flags_override_earlier_flags_and_environment(self):
        self.env["WORKMAN_DEV_RELAUNCH"] = "0"
        code, output = self.run_installer_completion("--no-cleanup", "--cleanup", "--no-relaunch", "--relaunch")
        self.assertEqual(code, 0, output)
        self.assertFalse(self.build.exists())
        self.assertFalse(self.dist.exists())
        self.assertEqual(self.launch_log.read_text(), "app\n")

    def test_installer_cleanup_failure_still_launches_and_returns_failure(self):
        self.env["CLEANUP_TEST_FAIL"] = "1"
        code, output = self.run_installer_completion()
        self.assertEqual(code, 7, output)
        self.assertTrue(self.dist.exists())
        self.assertEqual(self.launch_log.read_text(), "app\n")
        self.assert_installed_and_source_preserved()

    def test_explicit_cleanup_honors_custom_target_with_spaces(self):
        self.build = self.root / "custom build cache"
        self.build.mkdir()
        (self.build / "cache").write_text("generated")
        code, output = self.run_cleanup("clean")
        self.assertEqual(code, 0, output)
        self.assertFalse(self.build.exists())
        self.assertFalse(self.dist.exists())
        self.assertEqual(json.loads(self.log.read_text()), ["clean", "--manifest-path", str(self.repo / "Cargo.toml"), "--target-dir", str(self.build.resolve())])
        self.assertTrue((self.repo / "target").exists(), "Only the selected target is cleaned")
        self.assert_installed_and_source_preserved()

    def test_cleanup_ask_mode_noninteractive_does_not_consume_piped_yes_or_clean(self):
        code, output = self.run_cleanup()
        self.assertEqual(code, 0, output)
        self.assertIn("Use --cleanup", output)
        self.assertTrue(self.build.exists())
        self.assertTrue(self.dist.exists())
        self.assertFalse(self.log.exists())

    def test_flags_are_accepted_by_installer_help_without_installing(self):
        for flag in ["--cleanup", "--no-cleanup", "--relaunch", "--no-relaunch"]:
            result = subprocess.run(["bash", str(REPO / "scripts/dev-install.sh"), flag, "--help"],
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=5)
            self.assertEqual(result.returncode, 0, result.stdout.decode())
            self.assertIn(flag, result.stdout.decode())

    def test_explicit_keep_does_not_prompt_on_a_terminal(self):
        code, output = self.run_cleanup("keep", answer=b"y\n")
        self.assertEqual(code, 0, output)
        self.assertNotIn("[y/N]", output)
        self.assertTrue(self.build.exists())
        self.assertFalse(self.log.exists())

    def test_interactive_yes_cleans_after_showing_output_paths(self):
        code, output = self.run_cleanup(answer=b"y\n")
        self.assertEqual(code, 0, output)
        self.assertIn(str(self.build), output)
        self.assertFalse(self.build.exists())
        self.assertFalse(self.dist.exists())
        self.assert_installed_and_source_preserved()

    def test_interactive_default_no_and_eof_keep_cache(self):
        for answer in [b"\n", b"n\n", b"\x04"]:
            with self.subTest(answer=answer):
                code, output = self.run_cleanup(answer=answer)
                self.assertEqual(code, 0, output)
                self.assertTrue(self.build.exists())
                self.assertTrue(self.dist.exists())
                self.assertFalse(self.log.exists())

    def test_cargo_failure_preserves_frontend_output(self):
        self.env["CLEANUP_TEST_FAIL"] = "1"
        code, output = self.run_cleanup("clean")
        self.assertEqual(code, 7, output)
        self.assertTrue(self.build.exists())
        self.assertTrue(self.dist.exists())
        self.assertNotIn("Build output removed", output)
        self.assert_installed_and_source_preserved()

    def test_refuses_source_installed_data_and_root_as_cleanup_targets(self):
        for target in [self.repo, self.root, *self.installed, Path("/")]:
            with self.subTest(target=target):
                self.build = target
                code, output = self.run_cleanup("clean")
                self.assertEqual(code, 1, output)
                self.assertIn("cleanup refused", output)
                self.assertFalse(self.log.exists())
                self.assertTrue(self.dist.exists())
                self.assert_installed_and_source_preserved()

    def test_refuses_symlink_to_source(self):
        self.build = self.root / "unsafe target link"
        self.build.symlink_to(self.repo, target_is_directory=True)
        code, output = self.run_cleanup("clean")
        self.assertEqual(code, 1, output)
        self.assertFalse(self.log.exists())
        self.assert_installed_and_source_preserved()

    def test_refuses_installation_inside_frontend_output_before_cleaning_anything(self):
        self.installed[0] = self.dist / "installed binaries"
        self.installed[0].mkdir()
        (self.installed[0] / "keep-or-clean").write_text("fixture")
        code, output = self.run_cleanup("clean")
        self.assertEqual(code, 1, output)
        self.assertFalse(self.log.exists())
        self.assertTrue(self.build.exists())
        self.assert_installed_and_source_preserved()

    def test_relaunch_interactive_enter_and_yes_open_the_installed_cli(self):
        for answer in [b"\n", b"y\n", b"yes\n"]:
            with self.subTest(answer=answer):
                code, output = self.run_relaunch(answer=answer)
                self.assertEqual(code, 0, output)
                self.assertEqual(self.launch_log.read_text(), "app\n")
                self.assertIn("Workman Dev is running", output)
                self.launch_log.unlink()

    def test_relaunch_interactive_no_and_eof_leave_app_closed(self):
        for answer in [b"n\n", b"\x04"]:
            with self.subTest(answer=answer):
                code, output = self.run_relaunch(answer=answer)
                self.assertEqual(code, 0, output)
                self.assertFalse(self.launch_log.exists())
                self.assertIn("left closed", output)

    def test_relaunch_ask_mode_noninteractive_does_not_open_or_consume_piped_yes(self):
        code, output = self.run_relaunch()
        self.assertEqual(code, 0, output)
        self.assertFalse(self.launch_log.exists())
        self.assertNotIn("[Y/n]", output)
        self.assertIn("wrk-dev app", output)

    def test_relaunch_explicit_choices_bypass_prompt(self):
        code, output = self.run_relaunch("1")
        self.assertEqual(code, 0, output)
        self.assertEqual(self.launch_log.read_text(), "app\n")
        self.assertNotIn("[Y/n]", output)
        self.launch_log.unlink()
        code, output = self.run_relaunch("0", answer=b"y\n")
        self.assertEqual(code, 0, output)
        self.assertFalse(self.launch_log.exists())
        self.assertNotIn("[Y/n]", output)

    def test_relaunch_failure_is_reported_without_claiming_app_is_running(self):
        self.env["RELAUNCH_TEST_EXIT"] = "9"
        code, output = self.run_relaunch("1")
        self.assertEqual(code, 9, output)
        self.assertNotIn("Workman Dev is running", output)

    def test_relaunch_uses_installed_cli_after_cleanup_removes_build_output(self):
        code, output = self.run_cleanup("clean")
        self.assertEqual(code, 0, output)
        self.assertFalse(self.build.exists())
        code, output = self.run_relaunch("1")
        self.assertEqual(code, 0, output)
        self.assertEqual(self.launch_log.read_text(), "app\n")
        self.assert_installed_and_source_preserved()


if __name__ == "__main__":
    unittest.main()
