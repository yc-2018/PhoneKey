"""Offline checks for the presence and click-to-unlock state machines."""
import sys
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "app"))

from presence_key import controller as controller_module  # noqa: E402
from presence_key import presence as presence_module  # noqa: E402
from presence_key.config import DEFAULT_SETTINGS, Settings  # noqa: E402
from presence_key.controller import Controller  # noqa: E402

assert DEFAULT_SETTINGS.near_rssi == -72
assert DEFAULT_SETTINGS.far_rssi == -88
settings = Settings(near_rssi=-40, far_rssi=-30)
settings.normalize()
assert settings.far_rssi <= settings.near_rssi - 4


class FakeTime:
    def __init__(self):
        self.now = 0.0

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.now += seconds


class FakeAdvert:
    def __init__(self, rssi):
        self.rssi = rssi
        self.service_uuids = [presence_module.BEACON_UUID]
        self.manufacturer_data = {}


class FakeDevice:
    address = "AA:BB:CC:DD:EE:FF"


def tick_at(monitor, clock, moment):
    clock.now = moment
    monitor._tick()


settings = Settings(near_rssi=-60, far_rssi=-80, near_confirm=2.0, away_timeout=5.0)
changes = []
monitor = presence_module.PresenceMonitor(lambda: settings, lambda old, new: changes.append(new))
monitor._scanning = True
clock = FakeTime()
with patch.object(presence_module, "time", clock):
    monitor._on_advertisement(FakeDevice(), FakeAdvert(-50))
    tick_at(monitor, clock, 0.0)
    tick_at(monitor, clock, 1.9)
    assert monitor.snapshot()["state"] == presence_module.STATE_UNKNOWN
    tick_at(monitor, clock, 2.0)
    assert monitor.snapshot()["state"] == presence_module.STATE_NEAR
    tick_at(monitor, clock, 4.9)
    assert monitor.snapshot()["state"] == presence_module.STATE_NEAR
    tick_at(monitor, clock, 5.0)
    assert monitor.snapshot()["state"] == presence_module.STATE_AWAY
assert changes == [presence_module.STATE_NEAR, presence_module.STATE_AWAY]

never_seen = presence_module.PresenceMonitor(lambda: settings, lambda old, new: None)
never_seen._scanning = True
clock = FakeTime()
with patch.object(presence_module, "time", clock):
    tick_at(never_seen, clock, 100.0)
assert never_seen.snapshot()["state"] == presence_module.STATE_UNKNOWN


class FakePresence:
    def __init__(self):
        self.state = presence_module.STATE_UNKNOWN
        self.seen = True

    def snapshot(self):
        return {
            "state": self.state,
            "rssi": -50 if self.state == presence_module.STATE_NEAR else None,
            "smoothed": None,
            "last_seen_age": 0.0,
            "seen_since_start": self.seen,
            "scanning": True,
            "error": None,
            "address": "AA:BB:CC:DD:EE:FF",
        }

    def start(self):
        pass

    def stop(self):
        pass

    def reset(self):
        self.state = presence_module.STATE_UNKNOWN


fake_time = FakeTime()
fake_time.now = 1000.0
calls = {"lock": 0, "offer": [], "clear": 0}


def make_controller(presence, **overrides):
    values = dict(
        auto_lock=False,
        click_unlock=True,
        unlock_delay=1.5,
        lock_grace=3.0,
    )
    values.update(overrides)
    controller = Controller(Settings(**values))
    controller._presence = presence
    return controller


with patch.object(controller_module.lockstate, "is_locked", return_value=False), \
        patch.object(controller_module.lockstate, "is_remote_session", return_value=False), \
        patch.object(controller_module.lockstate, "lock_workstation", side_effect=lambda: calls.__setitem__("lock", calls["lock"] + 1) or True), \
        patch.object(controller_module, "save_settings", lambda settings: None), \
        patch.object(controller_module.secret, "has_secret", return_value=True), \
        patch.object(controller_module.secret, "load_secret", return_value="pw"), \
        patch.object(controller_module.credential, "account_name", return_value="yc"), \
        patch.object(controller_module.credential, "account_domain", return_value="."), \
        patch.object(controller_module.unlock, "cp_installed", return_value=True), \
        patch.object(controller_module.unlock, "send_unlock", side_effect=lambda *a, **k: calls["offer"].append(a) or (True, "OK")), \
        patch.object(controller_module.unlock, "clear_pending", side_effect=lambda *a, **k: calls.__setitem__("clear", calls["clear"] + 1) or (True, "OK")), \
        patch.object(controller_module.time, "time", fake_time.monotonic), \
        patch.object(controller_module.time, "monotonic", fake_time.monotonic), \
        patch.object(controller_module.time, "sleep", fake_time.sleep):
    fake = FakePresence()
    controller = make_controller(fake)

    # Locking with the phone nearby only prepares an unlock offer; the app
    # never submits the password by itself.
    fake.state = presence_module.STATE_NEAR
    controller._handle_event("presence", presence_module.STATE_NEAR)
    controller._handle_event("locked", True)
    controller._evaluate()
    assert calls["offer"] == []
    fake_time.now += 1.4
    controller._evaluate()
    assert calls["offer"] == []
    fake_time.now += 0.2
    controller._evaluate()
    assert len(calls["offer"]) == 1
    assert calls["offer"][0][0] == "yc"
    assert controller.snapshot()["unlock_offered"] is True
    assert calls["clear"] == 0

    # A steady near state must not offer repeatedly.
    fake_time.now += 30
    controller._evaluate()
    assert len(calls["offer"]) == 1

    # Leaving withdraws the offer so the lock screen can no longer unlock.
    fake.state = presence_module.STATE_AWAY
    controller._handle_event("presence", presence_module.STATE_AWAY)
    controller._evaluate()
    assert calls["clear"] == 1
    assert controller.snapshot()["unlock_offered"] is False

    # Returning offers the option again after the delay.
    fake_time.now += 1
    fake.state = presence_module.STATE_NEAR
    controller._handle_event("presence", presence_module.STATE_NEAR)
    controller._evaluate()
    fake_time.now += 1.6
    controller._evaluate()
    assert len(calls["offer"]) == 2
    assert calls["clear"] == 1

    # Unlocking the screen withdraws the offer.
    controller._handle_event("locked", False)
    controller._evaluate()
    assert calls["clear"] == 2
    assert controller.snapshot()["unlock_offered"] is False

    # Re-lock while near, then pause: the offer must be withdrawn.
    fake_time.now += 5
    controller._handle_event("locked", True)
    controller._evaluate()
    fake_time.now += 1.6
    controller._evaluate()
    assert len(calls["offer"]) == 3
    controller.set_paused(True)
    controller._evaluate()
    assert calls["clear"] == 3
    fake_time.now += 60
    controller._evaluate()
    assert len(calls["offer"]) == 3
    controller.set_paused(False)
    fake.state = presence_module.STATE_NEAR

    # Remote sessions suspend everything and withdraw any offer.
    fake_time.now += 1
    controller._handle_event("presence", presence_module.STATE_NEAR)
    controller._evaluate()
    fake_time.now += 1.6
    controller._evaluate()
    assert len(calls["offer"]) == 4
    controller._handle_event("remote", True)
    controller._evaluate()
    assert calls["clear"] == 4
    fake_time.now += 30
    controller._evaluate()
    assert len(calls["offer"]) == 4
    controller._handle_event("remote", False)

    # Auto-lock still works when enabled, and no offer is made while away.
    locks_before = calls["lock"]
    offers_before = len(calls["offer"])
    fake2 = FakePresence()
    fake2.state = presence_module.STATE_AWAY
    controller2 = make_controller(fake2, auto_lock=True)
    controller2._handle_event("presence", presence_module.STATE_AWAY)
    controller2._evaluate()
    fake_time.now += 2.9
    controller2._evaluate()
    assert calls["lock"] == locks_before
    fake_time.now += 0.2
    controller2._evaluate()
    assert calls["lock"] == locks_before + 1
    assert len(calls["offer"]) == offers_before

    # Disabled click-unlock never offers credentials.
    fake3 = FakePresence()
    fake3.state = presence_module.STATE_NEAR
    controller3 = make_controller(fake3, click_unlock=False)
    controller3._handle_event("presence", presence_module.STATE_NEAR)
    controller3._handle_event("locked", True)
    controller3._evaluate()
    fake_time.now += 10
    controller3._evaluate()
    assert len(calls["offer"]) == offers_before
    assert calls["clear"] == 4

    # Missing password or missing provider keeps the tile without a grant.
    fake4 = FakePresence()
    fake4.state = presence_module.STATE_NEAR
    controller4 = make_controller(fake4)
    controller4._handle_event("presence", presence_module.STATE_NEAR)
    controller4._handle_event("locked", True)
    with patch.object(controller_module.secret, "has_secret", return_value=False):
        controller4.invalidate_cp_cache()
        controller4._evaluate()
        fake_time.now += 10
        controller4._evaluate()
    with patch.object(controller_module.unlock, "cp_installed", return_value=False):
        controller4.invalidate_cp_cache()
        controller4._evaluate()
        fake_time.now += 10
        controller4._evaluate()
    assert len(calls["offer"]) == offers_before
    assert calls["clear"] == 4

    # Offer attempts are retried while the lock screen is still loading.
    fake5 = FakePresence()
    fake5.state = presence_module.STATE_NEAR
    controller5 = make_controller(fake5)
    controller5._handle_event("presence", presence_module.STATE_NEAR)
    controller5._handle_event("locked", True)
    attempts = []

    def fail_offer(*args, **kwargs):
        attempts.append(args)
        raise controller_module.unlock.UnlockError("pipe missing")

    with patch.object(controller_module.unlock, "send_unlock", side_effect=fail_offer):
        controller5._evaluate()
        fake_time.now += 1.6
        controller5._evaluate()
        assert len(attempts) == 1
        assert controller5.snapshot()["unlock_offered"] is False
        fake_time.now += 2.1
        controller5._evaluate()
        assert len(attempts) == 2

print("state machine checks passed")
