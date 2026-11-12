"""Host-side saved-login checks. Called only after the shell approval gate.

Only the named disposable Compose project and data folder are modified.
There are never two runtime owners on that data folder: cross-container
forget uses an exec-only api container while the gateway owns the runtime.
"""
import json
import pathlib
import subprocess
import sys
import time

COOKIE = "spec050_login=saved-fixture"
PAGE = "http://172.30.250.2:8000"


class Check:
    def __init__(self, project: str, compose: str, data: str):
        self.compose = ["docker", "compose", "--project-name", project, "--file", compose]
        self.data = pathlib.Path(data)
        self.profile = self.data / "browser/profile"
        self.role = "gateway"

    def command(self, *arguments: str, success: bool = True):
        result = subprocess.run(self.compose + list(arguments), capture_output=True, text=True, timeout=240)
        if success:
            assert result.returncode == 0, result.stdout + result.stderr
        return result

    def control(self, op: str, **fields):
        result = self.command("exec", "-T", self.role, "python3", "-I", "/fixtures/driver.py",
                              "client", json.dumps({"op": op, **fields}))
        value = json.loads(result.stdout)
        assert value["ok"], value
        return value

    def ready(self) -> None:
        deadline = time.monotonic() + 180
        while time.monotonic() < deadline:
            result = self.command("exec", "-T", self.role, "python3", "-I", "/fixtures/driver.py",
                                  "client", '{"op":"ready"}', success=False)
            if result.returncode == 0 and json.loads(result.stdout).get("ready"):
                return
            time.sleep(1)
        raise TimeoutError("fixture runtime did not become ready")

    def call(self, action: str, **fields) -> str:
        result = json.dumps(self.control("call", action={"action": action, **fields})["result"],
                            ensure_ascii=False)
        print(f"[{self.role}] {action} {fields}: {result[:300]}", flush=True)
        return result

    def open(self, page: str) -> str:
        return self.call("open", url=PAGE + page)

    def forget(self, role: str | None = None, success: bool = True):
        return self.command("exec", "-T", role or self.role, "shacs-bot", "--config",
                            "/check-data/config.json", "browser", "forget", "--yes", success=success)

    def disk(self):
        return self.control("disk")

    def closed(self) -> None:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if not [p for p in self.disk()["processes"] if p["name"] == "chromium"]:
                return
            time.sleep(0.1)
        raise TimeoutError("browser did not close within ten seconds")

    def chromium(self):
        children = [p for p in self.disk()["processes"] if p["name"] == "chromium"]
        assert len(children) == 1, children
        return children[0]

    def clean_close(self) -> None:
        facts = self.disk()
        chromium = self.chromium()
        root = next(arg.split("=", 1)[1].rsplit("/", 1)[0] for arg in chromium["argv"]
                    if arg.startswith("--disk-cache-dir="))
        self.call("close")
        self.closed()
        after = self.disk()
        assert root not in after["roots"], (facts, after)
        assert after["profile_exists"] and after["profile_mode"] == 0o700, after
        assert not after["singleton"], after

    def restart(self, persist: bool) -> None:
        self.command("stop", self.role)
        config_path = self.data / "config.json"
        config = json.loads(config_path.read_text())
        config["tools"]["browser"]["persistLogin"] = persist
        config_path.write_text(json.dumps(config))
        self.command("start", self.role)
        self.ready()

    def inventory(self):
        return sorted((str(p.relative_to(self.profile)), p.lstat().st_mode,
                       p.lstat().st_size, p.lstat().st_mtime_ns,
                       str(p.readlink()) if p.is_symlink() else "")
                      for p in self.profile.rglob("*"))

    def run(self) -> None:
        self.ready()
        self.open("/login")
        assert COOKIE in self.open("/whoami")
        argv = self.chromium()["argv"]
        assert "--user-data-dir=/check-data/browser/profile" in argv, argv
        assert "--password-store=basic" in argv, argv
        # Debian's Chromium launcher moves the page before its switches.
        assert [arg for arg in argv[1:] if not arg.startswith("--")] == ["about:blank"], argv
        assert not any(a.startswith("--disk-cache-size") for a in argv), argv
        cached = self.open("/cached")
        assert "fixed cache response" in cached, cached
        # Chromium flushes asynchronously; wait for an actual cache entry.
        deadline = time.monotonic() + 10
        while not any(self.disk()["cache_files"].values()):
            assert time.monotonic() < deadline, ("no HTTP cache file", self.disk())
            time.sleep(0.1)
        assert not self.disk()["profile_cache"]
        self.clean_close()
        assert COOKIE in self.open("/whoami")
        self.clean_close()
        self.command("restart", self.role)
        self.ready()
        assert COOKIE in self.open("/whoami")
        self.open("/cached")
        before_kill = self.disk()
        self.command("kill", self.role)
        self.command("start", self.role)
        self.ready()
        # PRD 000 reports the killed session once before allowing a fresh
        # browser. A mere absence of profile_locked would also pass on this
        # notice and would not prove that Chromium started.
        assert "browser_closed" in self.open("/whoami")
        assert COOKIE in self.open("/whoami")
        after_kill = self.disk()
        retained = set(before_kill["roots"]) & set(after_kill["roots"])
        assert retained, (before_kill, after_kill)
        print("SIGKILL retained roots/cache:", sorted(retained),
              {root: after_kill["cache_files"][root] for root in retained})
        self.clean_close()

        self.command("exec", "-T", self.role, "python3", "-c",
                     "from pathlib import Path; "
                     "Path('/check-data/browser/profile/SingletonLock').symlink_to('different-host-4242')")
        before = self.inventory()
        assert "browser_profile_locked" in self.open("/whoami")
        refused = self.forget(success=False)
        assert refused.returncode != 0 and "다른 곳에서 잠겨" in refused.stdout + refused.stderr
        assert self.inventory() == before
        self.command("exec", "-T", self.role, "python3", "-c",
                     "from pathlib import Path; Path('/check-data/browser/profile/SingletonLock').unlink()")
        self.forget()
        assert not self.profile.exists()
        self.open("/login")
        # Exec-only api shares the volume and kernel but has a different hostname.
        api_container = self.command("run", "-d", "--no-deps", "--entrypoint",
                                     "sleep", "api", "infinity").stdout.strip().splitlines()[-1]
        assert api_container, "no exec-only api container"
        other = subprocess.run(["docker", "exec", api_container, "shacs-bot", "--config",
                                "/check-data/config.json", "browser", "forget", "--yes"],
                               capture_output=True, text=True, timeout=30)
        assert other.returncode == 0, other.stdout + other.stderr
        assert "브라우저를 닫았습니다." in other.stdout and not self.profile.exists()
        self.closed()
        assert "browser_closed" in self.call("look")
        assert COOKIE not in self.open("/whoami")
        self.call("close")
        self.command("stop", self.role)
        subprocess.run(["docker", "rm", "-f", api_container], check=True, timeout=30)
        # Same real construction path with the production serve command.
        self.role = "api"
        self.command("up", "-d", "api")
        self.ready()
        self.open("/login")
        self.clean_close()
        assert COOKIE in self.open("/whoami")
        self.clean_close()

        # Off: saved bytes are not touched; temporary browsers still close.
        before = self.inventory()
        (self.data / "browser/profile.lock").unlink(missing_ok=True)
        self.restart(False)
        self.open("/whoami")
        argv = self.chromium()["argv"]
        assert any(a.startswith("--user-data-dir=/tmp/shacs-browser-") for a in argv), argv
        assert not any(a.startswith(("--disk-cache-dir", "--password-store")) for a in argv), argv
        assert self.inventory() == before
        assert (self.data / "browser/active.lock").exists()
        assert not (self.data / "browser/profile.lock").exists()
        result = self.forget()
        assert "브라우저를 닫았습니다." in result.stdout and "로그인 기록을 지웠습니다." in result.stdout
        assert not self.profile.exists()
        assert "browser_closed" in self.call("look")
        self.open("/whoami")
        result = self.forget()
        assert "브라우저를 닫았습니다." in result.stdout and "지울 로그인 기록이 없습니다." in result.stdout
        assert "browser_closed" in self.call("look")

        # Permission checks use an actual provider-issued exec in bypass mode.
        self.open("/whoami")
        browser = self.data / "browser"
        lock = browser / "active.lock"
        identities = (browser.stat().st_ino, lock.stat().st_ino)
        for command in ["rm /check-data/browser/active.lock",
                        "mv /check-data/browser /check-data/renamed-browser"]:
            answer = json.dumps(self.control("exec", command=command), ensure_ascii=False)
            assert "브라우저 lock 파일은 바꿀 수 없습니다." in answer, answer
            assert identities == (browser.stat().st_ino, lock.stat().st_ino)
            self.chromium()
        self.call("close")

        for persist in [False, True]:
            self.restart(persist)
            for kind in ["lock", "folder"]:
                self.open("/whoami")
                match kind:
                    case "lock":
                        fresh = browser / "replacement.lock"
                        fresh.touch()
                        fresh.replace(lock)
                    case "folder":
                        moved = self.data / f"old-browser-{persist}"
                        browser.rename(moved)
                        browser.mkdir(mode=0o700)
                self.closed()
                assert self.control("inspect")["browser"]["reason"] == "browser_lock_replaced"
                assert "browser_closed" in self.call("look")
        print("Saved-login real Chromium, gateway, serve and cross-container checks passed.")


if __name__ == "__main__":
    check = Check(*sys.argv[1:])
    try:
        check.run()
    except Exception:
        logs = check.command("logs", "--no-color", "--tail", "80", success=False)
        print("FAILED container logs:", logs.stdout, logs.stderr, flush=True)
        for op in ("disk", "log_tail"):
            try:
                print(f"FAILED {op}: {check.control(op)}", flush=True)
            except (AssertionError, subprocess.SubprocessError, OSError) as error:
                print(f"FAILED {op} unavailable: {error}", flush=True)
        raise
