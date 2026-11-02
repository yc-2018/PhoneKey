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


class FakePhoneAdvert:
    def __init__(self, payload_hex, rssi=-50):
        self.rssi = rssi
        self.service_uuids = []
        self.manufacturer_data = {}
        self.service_data = {
            presence_module.PHONE_ADV_UUID: bytes.fromhex(payload_hex)
        }


class FakePhoneDevice:
    address = "AA:BB:CC:DD:EE:BC"


bound_phone = Settings(
    phone_adv_prefix="030101aabbccddee",
    phone_adv_address=FakePhoneDevice.address,
)
phone_monitor = presence_module.PresenceMonitor(lambda: bound_phone, lambda old, new: None)
assert phone_monitor._classify(
    FakePhoneDevice(), FakePhoneAdvert("030101aabbccddeec906")
)[0] == "phone"
# Bluetooth-off / offline-finding frames from the same phone must not count.
assert phone_monitor._classify(
    FakePhoneDevice(), FakePhoneAdvert("0341012660a79392c906")
) is None

bound_phone.phone_adv_prefix = ""
assert phone_monitor._classify(
    FakePhoneDevice(), FakePhoneAdvert("0341012660a79392c906")
)[0] == "phone"


def phone_state_after(auto_lock, seconds):
    phone_settings = Settings(
        near_rssi=-60,
        far_rssi=-80,
        near_confirm=2.0,
        away_timeout=12.0,
        auto_lock=auto_lock,
        phone_adv_prefix="030101aabbccddee",
        phone_adv_address=FakePhoneDevice.address,
    )
    monitor = presence_module.PresenceMonitor(
        lambda: phone_settings, lambda old, new: None
    )
    monitor._scanning = True
    clock = FakeTime()
    with patch.object(presence_module, "time", clock):
        monitor._on_advertisement(
            FakePhoneDevice(), FakePhoneAdvert("030101aabbccddeec906")
        )
        monitor._tick()
        tick_at(monitor, clock, 2.0)
        assert monitor.snapshot()["state"] == presence_module.STATE_NEAR
        tick_at(monitor, clock, seconds)
        return monitor.snapshot()["state"]


# Click mode follows the user's away timeout; auto-lock keeps the 30s floor.
assert phone_state_after(auto_lock=False, seconds=12.0) == presence_module.STATE_AWAY
assert phone_state_after(auto_lock=True, seconds=12.0) == presence_module.STATE_NEAR
assert phone_state_after(auto_lock=True, seconds=29.9) == presence_module.STATE_NEAR
assert phone_state_after(auto_lock=True, seconds=30.0) == presence_module.STATE_AWAY

# Manual refresh clears the state and asks the scan loop to restart.
refresh_settings = Settings(
    near_rssi=-60,
    far_rssi=-80,
    near_confirm=2.0,
    away_timeout=12.0,
    phone_adv_prefix="030101aabbccddee",
    phone_adv_address=FakePhoneDevice.address,
)
refresh_monitor = presence_module.PresenceMonitor(
    lambda: refresh_settings, lambda old, new: None
)
refresh_monitor._scanning = True
clock = FakeTime()
with patch.object(presence_module, "time", clock):
    refresh_monitor._on_advertisement(
        FakePhoneDevice(), FakePhoneAdvert("030101aabbccddeec906")
    )
    refresh_monitor._tick()
    clock.now = 2.0
    refresh_monitor._tick()
assert refresh_monitor.snapshot()["state"] == presence_module.STATE_NEAR
with patch.object(refresh_monitor, "start"):
    refresh_monitor.refresh()
assert refresh_monitor._restart.is_set()
assert refresh_monitor.snapshot()["state"] == presence_module.STATE_UNKNOWN


# check_near only accepts a fresh advertisement above the near threshold.
check_settings = Settings(
    near_rssi=-60,
    far_rssi=-80,
    near_confirm=2.0,
    away_timeout=12.0,
    phone_adv_enabled=False,
)
check_monitor = presence_module.PresenceMonitor(
    lambda: check_settings, lambda old, new: None
)
check_monitor._scanning = True
clock = FakeTime()
with patch.object(presence_module, "time", clock):
    assert check_monitor.check_near() is False
    check_monitor._on_advertisement(FakeDevice(), FakeAdvert(-50))
    assert check_monitor.check_near() is True
    clock.now = 6.1
    assert check_monitor.check_near() is False

# Phone-mode checks accept one full advertisement cycle (about 15s).
phone_check_settings = Settings(
    near_rssi=-60,
    far_rssi=-80,
    near_confirm=2.0,
    away_timeout=12.0,
    phone_adv_enabled=True,
    phone_adv_prefix="030101aabbccddee",
)
phone_check_monitor = presence_module.PresenceMonitor(
    lambda: phone_check_settings, lambda old, new: None
)
phone_check_monitor._scanning = True
clock = FakeTime()
with patch.object(presence_module, "time", clock):
    phone_check_monitor._on_advertisement(
        FakePhoneDevice(), FakePhoneAdvert("030101aabbccddeec906")
    )
    clock.now = 15.9
    assert phone_check_monitor.check_near() is True
    clock.now = 16.1
    assert phone_check_monitor.check_near() is False

# In phone mode without a bound phone, an unrelated beacon must not verify.
unbound_monitor = presence_module.PresenceMonitor(
    lambda: Settings(phone_adv_enabled=True), lambda old, new: None
)
unbound_monitor._scanning = True
clock = FakeTime()
with patch.object(presence_module, "time", clock):
    unbound_monitor._on_advertisement(FakeDevice(), FakeAdvert(-50))
    assert unbound_monitor.check_near() is False

# verify_near polls until its deadline (or until scanning stops).
verify_monitor = presence_module.PresenceMonitor(
    lambda: check_settings, lambda old, new: None
)
verify_monitor._scanning = True
clock = FakeTime()
with patch.object(presence_module, "time", clock):
    assert verify_monitor.verify_near(timeout=0.3) is False
    assert clock.now >= 0.3
    verify_monitor._on_advertisement(FakeDevice(), FakeAdvert(-50))
    assert verify_monitor.verify_near(timeout=1.0) is True


class FakePresence:
    def __init__(self):
        self.state = presence_module.STATE_UNKNOWN
        self.seen = True
        self.scanning = True
        self.last_seen_age = 0.0
        self.near_check = True
        self.check_calls = 0
        self.refresh_calls = 0

    def snapshot(self):
        return {
            "state": self.state,
            "rssi": -50 if self.state == presence_module.STATE_NEAR else None,
            "smoothed": None,
            "last_seen_age": self.last_seen_age,
            "seen_since_start": self.seen,
            "scanning": self.scanning,
            "error": None,
            "address": "AA:BB:CC:DD:EE:FF",
        }

    def check_near(self, max_age=presence_module.VERIFY_FRESH_WINDOW):
        self.check_calls += 1
        return self.near_check

    def start(self):
        pass

    def stop(self):
        pass

    def reset(self):
        self.state = presence_module.STATE_UNKNOWN

    def refresh(self):
        self.refresh_calls += 1
        self.reset()


fake_time = FakeTime()
fake_time.now = 1000.0
calls = {"lock": 0, "offer": [], "clear": 0, "status": []}


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
        patch.object(controller_module.unlock, "send_presence_status", side_effect=lambda near, **k: calls["status"].append(near) or (True, "OK")), \
        patch.object(controller_module.unlock, "clear_pending", side_effect=lambda *a, **k: calls.__setitem__("clear", calls["clear"] + 1) or (True, "OK")), \
        patch.object(controller_module.time, "time", fake_time.monotonic), \
        patch.object(controller_module.time, "monotonic", fake_time.monotonic), \
        patch.object(controller_module.time, "sleep", fake_time.sleep):
    fake = FakePresence()
    controller = make_controller(fake)

    # Entering the lock screen checks proximity once and records the result.
    fake.state = presence_module.STATE_NEAR
    fake.near_check = True
    controller._handle_event("presence", presence_module.STATE_NEAR)
    controller._handle_event("locked", True)
    assert fake.check_calls == 1
    assert controller.snapshot()["lock_check"] == presence_module.STATE_NEAR

    # The tile appears after the delay and the app never submits by itself.
    controller._evaluate()
    assert calls["offer"] == []
    fake_time.now += 1.4
    controller._evaluate()
    assert calls["offer"] == []

    # The phone leaving before the delay does not cancel the offer; the
    # click-time check is what protects the password.
    fake.state = presence_module.STATE_AWAY
    fake.near_check = False
    controller._handle_event("presence", presence_module.STATE_AWAY)
    controller._evaluate()
    assert calls["offer"] == []
    fake_time.now += 0.2
    controller._evaluate()
    assert len(calls["offer"]) == 1
    assert calls["offer"][0][0] == "yc"
    assert calls["status"] == [True]
    assert controller.snapshot()["unlock_offered"] is True
    assert calls["clear"] == 0

    # A steady offer is not repeated.
    fake_time.now += 30
    controller._evaluate()
    assert len(calls["offer"]) == 1

    # Unlocking withdraws the offer and resets the entry check.
    controller._handle_event("locked", False)
    controller._evaluate()
    assert calls["clear"] == 1
    assert controller.snapshot()["unlock_offered"] is False
    assert controller.snapshot()["lock_check"] is None

    # Re-lock while away still offers the tile and reports the away status.
    fake_time.now += 5
    fake.state = presence_module.STATE_AWAY
    fake.near_check = False
    controller._handle_event("presence", presence_module.STATE_AWAY)
    controller._handle_event("locked", True)
    assert controller.snapshot()["lock_check"] == presence_module.STATE_AWAY
    controller._evaluate()
    fake_time.now += 1.6
    controller._evaluate()
    assert len(calls["offer"]) == 2
    assert calls["status"][-1] is False

    # Pause withdraws; resume offers again after the delay.
    controller.set_paused(True)
    controller._evaluate()
    assert calls["clear"] == 2
    fake_time.now += 60
    controller._evaluate()
    assert len(calls["offer"]) == 2
    controller.set_paused(False)
    fake_time.now += 1
    controller._evaluate()
    fake_time.now += 1.6
    controller._evaluate()
    assert len(calls["offer"]) == 3

    # Remote sessions suspend everything and withdraw any offer.
    controller._handle_event("remote", True)
    controller._evaluate()
    assert calls["clear"] == 3
    fake_time.now += 30
    controller._evaluate()
    assert len(calls["offer"]) == 3
    controller._handle_event("remote", False)
    fake_time.now += 1
    controller._evaluate()
    fake_time.now += 1.6
    controller._evaluate()
    assert len(calls["offer"]) == 4

    # Auto-lock still works, and the tile is offered afterwards even though
    # the phone is away.
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
    assert controller2.snapshot()["locked"] is True
    controller2._evaluate()
    fake_time.now += 1.6
    controller2._evaluate()
    assert len(calls["offer"]) == offers_before + 1

    # Disabled click-unlock never offers credentials.
    fake3 = FakePresence()
    controller3 = make_controller(fake3, click_unlock=False)
    controller3._handle_event("locked", True)
    controller3._evaluate()
    fake_time.now += 10
    controller3._evaluate()
    assert len(calls["offer"]) == offers_before + 1

    # Missing password or missing provider keeps the tile without a grant.
    fake4 = FakePresence()
    controller4 = make_controller(fake4)
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
    assert len(calls["offer"]) == offers_before + 1

    # Offer attempts are retried while the lock screen is still loading.
    fake5 = FakePresence()
    controller5 = make_controller(fake5)
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

    # The offer no longer depends on scanning state or freshness; the click
    # pipeline re-verifies and fails closed on its own.
    fake6 = FakePresence()
    fake6.scanning = False
    fake6.last_seen_age = 60.0
    controller6 = make_controller(fake6)
    controller6._handle_event("locked", True)
    controller6._evaluate()
    fake_time.now += 1.6
    controller6._evaluate()
    assert controller6.snapshot()["unlock_offered"] is True
    clears_before = calls["clear"]
    fake6.scanning = False
    controller6._evaluate()
    assert controller6.snapshot()["unlock_offered"] is True
    assert calls["clear"] == clears_before

    # Resuming from sleep restarts scanning and re-sends the offer, because
    # LogonUI may have restarted and lost the pending credentials.
    fake7 = FakePresence()
    controller7 = make_controller(fake7)
    controller7._handle_event("locked", True)
    controller7._evaluate()
    fake_time.now += 1.6
    controller7._evaluate()
    assert controller7.snapshot()["unlock_offered"] is True
    clears_before = calls["clear"]
    offers_before = len(calls["offer"])
    controller7._on_resume()
    assert fake7.refresh_calls == 1
    assert controller7.snapshot()["unlock_offered"] is False
    controller7._evaluate()
    fake_time.now += 1.6
    controller7._evaluate()
    assert controller7.snapshot()["unlock_offered"] is True
    assert len(calls["offer"]) == offers_before + 1
    assert calls["clear"] == clears_before

print("state machine checks passed")
