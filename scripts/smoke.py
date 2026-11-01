"""Exercise the embedded release binary from an empty working directory."""
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import urllib.error
import urllib.request

binary = Path(__file__).resolve().parent.parent / "target/release/smartthermweb"
with tempfile.TemporaryDirectory(prefix="smarttherm-smoke-") as directory:
    env = dict(os.environ, SMARTTHERM_BIND="127.0.0.1:0", SMARTTHERM_PROVIDER="simulator",
               SMARTTHERM_SIM_CONFIG='{"scenario":"normal","ack_ms":40,"apply_ms":120}')
    process = subprocess.Popen([str(binary)], cwd=directory, env=env, stdout=subprocess.PIPE, text=True)
    try:
        line = process.stdout.readline().strip()
        assert "http://" in line, line
        base = line.split("http://", 1)[1]
        base = "http://" + base

        def request(path, data=None):
            body = None if data is None else json.dumps(data).encode()
            req = urllib.request.Request(base + path, data=body, method="GET" if data is None else "PUT",
                                         headers={} if body is None else {"Content-Type": "application/json"})
            return urllib.request.urlopen(req, timeout=12)

        assert json.load(request("/api/health"))["status"] == "ok"
        html = request("/").read().decode()
        assets = re.findall(r'(?:src|href)="(/assets/[^" ]+)"', html)
        assert len(assets) >= 2, "Compiled frontend was not embedded"
        for asset in assets:
            assert request(asset).status == 200
        try:
            request("/api/not-found")
            raise AssertionError("Unknown API unexpectedly succeeded")
        except urllib.error.HTTPError as error:
            assert error.code == 404 and error.headers["Content-Type"] == "application/json"

        def states(stream):
            while True:
                line = stream.readline()
                assert line, "SSE stream closed unexpectedly"
                if line.startswith(b"data: "):
                    yield json.loads(line[6:])

        for scenario, target, intermediate in [("normal", 230, "accepted_by_cloud"), ("reply_lost", 225, "transport_failed")]:
            with request("/api/simulator", {"scenario": scenario, "ack_ms": 40, "apply_ms": 120}):
                pass
            with request("/api/events") as stream:
                events = states(stream)
                next(events)
                with request("/api/thermostat/target", {"target_temperature": target}) as response:
                    assert response.status == 202
                    command_id = json.load(response)["command"]["id"]
                phases = []
                for state in events:
                    command = state["command"]
                    if command and command["id"] == command_id:
                        phases.append(command["phase"])
                        if command["phase"] == "confirmed":
                            assert state["confirmed"]["target_temperature"] == target
                            break
                assert intermediate in phases, phases
                print(scenario + ": " + " -> ".join(dict.fromkeys(phases)))

        with request("/api/events") as stream:
            events = states(stream)
            next(events)
            for scenario, expected in [("offline", "offline"), ("normal", "online")]:
                with request("/api/simulator", {"scenario": scenario, "ack_ms": 40, "apply_ms": 120}):
                    pass
                for state in events:
                    if state["connectivity"] == expected:
                        break
        print("PASS: standalone embedded assets, API 404, SSE lifecycle, lost reply, offline/reconnect")
    finally:
        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
