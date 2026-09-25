#!/usr/bin/env python3
"""CalDAV prototype for udo's timetable: talk to iCloud the way udo would.

Raw HTTP + XML on purpose (no CalDAV library), so every step maps 1:1 to
what the Rust version has to do.

    list                    calendars + subscriptions (with their ICS link)
    busy  -c Uni,Personal   busy times in the next days (recurring expanded)
    free  -c Uni,Personal   free time = window - exclusions - busy (+ buffer)
    write-test -c udo       put one test block into the udo calendar
    cleanup                 delete the test blocks written by write-test
    selftest                offline check of expansion + free-time logic

Credentials (never stored in files):
    ICLOUD_USER             Apple ID (e-mail)
    ICLOUD_APP_PASSWORD     app-specific password (appleid.apple.com), or
                            macOS Keychain item: service "udo-icloud",
                            account = your Apple ID, or a prompt.
"""

from __future__ import annotations

import argparse
import getpass
import json
import os
import subprocess
import sys
import uuid
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from datetime import date, datetime, time, timedelta, timezone
from pathlib import Path
from urllib.parse import urljoin

import icalendar
import recurring_ical_events
import requests

ICLOUD = "https://caldav.icloud.com/"
NS = {
    "d": "DAV:",
    "c": "urn:ietf:params:xml:ns:caldav",
    "cs": "http://calendarserver.org/ns/",
    "ic": "http://apple.com/ns/ical/",
}
WRITTEN = Path(__file__).with_name(".written.json")  # hrefs of test blocks


# --------------------------------------------------------------------------
# HTTP / CalDAV
# --------------------------------------------------------------------------


def credentials() -> tuple[str, str]:
    user = os.environ.get("ICLOUD_USER") or input("Apple ID: ").strip()
    pw = os.environ.get("ICLOUD_APP_PASSWORD")
    if not pw and sys.platform == "darwin":
        r = subprocess.run(
            ["security", "find-generic-password", "-a", user, "-s", "udo-icloud", "-w"],
            capture_output=True,
            text=True,
        )
        pw = r.stdout.strip() if r.returncode == 0 else None
    if not pw:
        pw = getpass.getpass("App-specific password: ")
    return user, pw


class Dav:
    def __init__(self, user: str, pw: str):
        self.s = requests.Session()
        self.s.auth = (user, pw)
        self.s.headers["User-Agent"] = "udo-caldav-prototype"

    def request(self, method: str, url: str, body: str | None = None, depth: str | None = None,
                **headers: str) -> requests.Response:
        h = dict(headers)
        if body is not None:
            h.setdefault("Content-Type", "application/xml; charset=utf-8")
        if depth is not None:
            h["Depth"] = depth
        r = self.s.request(method, url, data=body.encode() if body else None, headers=h)
        if r.status_code == 401:
            sys.exit("401 Unauthorized: check Apple ID / app-specific password")
        r.raise_for_status()
        return r

    def propfind(self, url: str, props: str, depth: str) -> list[tuple[str, ET.Element]]:
        """(absolute href, <d:prop> of the 200 propstat) per response."""
        body = (
            '<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" '
            'xmlns:cs="http://calendarserver.org/ns/" xmlns:ic="http://apple.com/ns/ical/">'
            f"<d:prop>{props}</d:prop></d:propfind>"
        )
        root = ET.fromstring(self.request("PROPFIND", url, body, depth).content)
        out = []
        for resp in root.findall("d:response", NS):
            href = urljoin(url, resp.findtext("d:href", "", NS))
            for ps in resp.findall("d:propstat", NS):
                if "200" in ps.findtext("d:status", "", NS):
                    out.append((href, ps.find("d:prop", NS)))
        return out


@dataclass
class Calendar:
    name: str
    href: str
    subscribed: bool
    source: str | None  # ICS link of a subscription


def discover(dav: Dav) -> list[Calendar]:
    # 1. who am I?  2. where are my calendars?  3. list them (Depth 1)
    [(_, p)] = dav.propfind(ICLOUD, "<d:current-user-principal/>", "0")
    principal = urljoin(ICLOUD, p.findtext("d:current-user-principal/d:href", "", NS))
    [(_, p)] = dav.propfind(principal, "<c:calendar-home-set/>", "0")
    home = urljoin(principal, p.findtext("c:calendar-home-set/d:href", "", NS))

    cals = []
    for href, p in dav.propfind(home, "<d:displayname/><d:resourcetype/><cs:source/>", "1"):
        rt = p.find("d:resourcetype", NS)
        if rt is None:
            continue
        is_cal = rt.find("c:calendar", NS) is not None
        is_sub = rt.find("cs:subscribed", NS) is not None
        if not (is_cal or is_sub):
            continue  # inbox, outbox, notifications, the home itself
        src = p.findtext("cs:source/d:href", None, NS)
        cals.append(Calendar(p.findtext("d:displayname", "?", NS), href, is_sub, src))
    return cals


def pick(cals: list[Calendar], names: str) -> list[Calendar]:
    wanted = [n.strip() for n in names.split(",") if n.strip()]
    by_name = {c.name.casefold(): c for c in cals}
    missing = [n for n in wanted if n.casefold() not in by_name]
    if missing:
        sys.exit(f"unknown calendar(s): {missing}; have: {[c.name for c in cals]}")
    return [by_name[n.casefold()] for n in wanted]


def utc(dt: datetime) -> str:
    """CalDAV time-range format: UTC, basic ISO."""
    return dt.astimezone(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def fetch_ics(dav: Dav, cal: Calendar, start: datetime, end: datetime) -> list[bytes]:
    """Raw iCalendar documents of `cal` touching [start, end)."""
    if cal.subscribed:
        # subscription: iCloud only knows the link; download the feed ourselves
        url = (cal.source or "").replace("webcal://", "https://", 1)
        r = requests.get(url, timeout=30)
        r.raise_for_status()
        return [r.content]
    # normal calendar: server-side time-range filter; recurring events come back
    # as master (+ overrides) and are expanded client-side like subscriptions
    body = (
        '<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">'
        "<d:prop><c:calendar-data/></d:prop>"
        '<c:filter><c:comp-filter name="VCALENDAR"><c:comp-filter name="VEVENT">'
        f'<c:time-range start="{utc(start)}" end="{utc(end)}"/>'
        "</c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"
    )
    root = ET.fromstring(dav.request("REPORT", cal.href, body, "1").content)
    return [e.text.encode() for e in root.iter("{urn:ietf:params:xml:ns:caldav}calendar-data") if e.text]


# --------------------------------------------------------------------------
# Busy / free logic (no network; this is what `selftest` covers)
# --------------------------------------------------------------------------


@dataclass
class Busy:
    start: datetime
    end: datetime
    title: str
    source: str


def local(v) -> datetime:
    """date / naive / aware -> aware local datetime."""
    if isinstance(v, datetime):
        return v.astimezone() if v.tzinfo else v.replace(tzinfo=datetime.now().astimezone().tzinfo)
    return datetime.combine(v, time.min).astimezone()


def busy_from_ics(docs: list[bytes], start: datetime, end: datetime, source: str) -> list[Busy]:
    out = []
    for raw in docs:
        cal = icalendar.Calendar.from_ical(raw)
        for ev in recurring_ical_events.of(cal).between(start, end):
            if str(ev.get("TRANSP", "OPAQUE")).upper() == "TRANSPARENT":
                continue  # marked "free" -> doesn't block
            s = ev["DTSTART"].dt
            if "DTEND" in ev:
                e = ev["DTEND"].dt
            elif "DURATION" in ev:
                e = s + ev["DURATION"].dt
            else:  # RFC 5545: all-day without end = 1 day, timed = instant
                e = s + timedelta(days=0 if isinstance(s, datetime) else 1)
            # all-day events (plain dates) block the whole day(s)
            out.append(Busy(local(s), local(e), str(ev.get("SUMMARY", "")), source))
    return sorted(out, key=lambda b: b.start)


@dataclass
class Rules:
    day_start: time = time(8, 0)
    day_end: time = time(22, 0)
    weekends: bool = False
    buffer: timedelta = timedelta(minutes=15)
    min_slot: timedelta = timedelta(minutes=30)


def free_slots(busy: list[Busy], start: datetime, days: int, rules: Rules) -> list[tuple[datetime, datetime]]:
    """Free time per day = allowed window - (busy +/- buffer), >= min_slot."""
    blocked = sorted((b.start - rules.buffer, b.end + rules.buffer) for b in busy)
    out = []
    for i in range(days):
        day = (start + timedelta(days=i)).date()
        if not rules.weekends and day.weekday() >= 5:
            continue
        lo = datetime.combine(day, rules.day_start).astimezone()
        hi = datetime.combine(day, rules.day_end).astimezone()
        lo = max(lo, start)  # today: not before now
        cur = lo
        for bs, be in blocked:
            if be <= cur or bs >= hi:
                continue
            if bs > cur:
                out.append((cur, min(bs, hi)))
            cur = max(cur, be)
            if cur >= hi:
                break
        if cur < hi:
            out.append((cur, hi))
    return [(a, b) for a, b in out if b - a >= rules.min_slot]


# --------------------------------------------------------------------------
# Output helpers
# --------------------------------------------------------------------------


def fmt(dt: datetime) -> str:
    return dt.strftime("%a %d.%m %H:%M")


def dur(td: timedelta) -> str:
    m = int(td.total_seconds() // 60)
    return f"{m // 60}h{m % 60:02d}" if m >= 60 else f"{m}m"


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------


def cmd_list(dav: Dav, _args) -> None:
    for c in discover(dav):
        kind = "subscription" if c.subscribed else "calendar"
        print(f"{c.name:<30} {kind:<13} {c.source or c.href}")


def collect_busy(dav: Dav, args, start: datetime, end: datetime) -> list[Busy]:
    cals = discover(dav)
    busy = []
    for c in pick(cals, args.calendars):
        busy += busy_from_ics(fetch_ics(dav, c, start, end), start, end, c.name)
    return sorted(busy, key=lambda b: b.start)


def cmd_busy(dav: Dav, args) -> None:
    start = datetime.now().astimezone()
    for b in collect_busy(dav, args, start, start + timedelta(days=args.days)):
        print(f"{fmt(b.start)} - {b.end:%H:%M}  [{b.source}] {b.title}")


def cmd_free(dav: Dav, args) -> None:
    start = datetime.now().astimezone()
    busy = collect_busy(dav, args, start, start + timedelta(days=args.days))
    show_free(free_slots(busy, start, args.days, rules_from(args)))


def show_free(slots) -> None:
    total = timedelta()
    for a, b in slots:
        total += b - a
        print(f"{fmt(a)} - {b:%H:%M}  ({dur(b - a)})")
    print(f"free in total: {dur(total)}")


def rules_from(args) -> Rules:
    return Rules(
        day_start=time.fromisoformat(args.day_start),
        day_end=time.fromisoformat(args.day_end),
        weekends=args.weekends,
        buffer=timedelta(minutes=args.buffer),
        min_slot=timedelta(minutes=args.min),
    )


def cmd_write_test(dav: Dav, args) -> None:
    [cal] = pick(discover(dav), args.calendars)
    if cal.subscribed:
        sys.exit("can't write into a subscription")
    if input(f"write one test block into '{cal.name}'? [y/N] ").lower() != "y":
        return
    start = datetime.combine(date.today() + timedelta(days=1), time(10, 0)).astimezone()
    uid = f"udo-proto-{uuid.uuid4()}@udo"
    ev = icalendar.Event()
    ev.add("uid", uid)
    ev.add("summary", "udo: test block")
    ev.add("dtstart", start)
    ev.add("dtend", start + timedelta(minutes=30))
    ev.add("dtstamp", datetime.now().astimezone())
    doc = icalendar.Calendar()
    doc.add("prodid", "-//udo//caldav prototype//EN")
    doc.add("version", "2.0")
    doc.add_component(ev)
    href = urljoin(cal.href, f"{uid}.ics")
    # If-None-Match: * -> never overwrite an existing event
    dav.request("PUT", href, doc.to_ical().decode(), None,
                **{"Content-Type": "text/calendar; charset=utf-8", "If-None-Match": "*"})
    hrefs = json.loads(WRITTEN.read_text()) if WRITTEN.exists() else []
    WRITTEN.write_text(json.dumps(hrefs + [href], indent=2))
    print(f"written {fmt(start)} (30m) -> {href}")


def cmd_cleanup(dav: Dav, _args) -> None:
    hrefs = json.loads(WRITTEN.read_text()) if WRITTEN.exists() else []
    for href in hrefs:
        r = dav.s.delete(href)
        print(f"{r.status_code} {href}")
    WRITTEN.unlink(missing_ok=True)


SAMPLE = b"""BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//udo//selftest//EN
BEGIN:VEVENT
UID:lecture
SUMMARY:Lecture (weekly Mon+Wed 10-12, not on 2026-10-07)
DTSTART;TZID=Europe/Vienna:20261005T100000
DTEND;TZID=Europe/Vienna:20261005T120000
RRULE:FREQ=WEEKLY;BYDAY=MO,WE
EXDATE;TZID=Europe/Vienna:20261007T100000
END:VEVENT
BEGIN:VEVENT
UID:birthday
SUMMARY:All-day (blocks)
DTSTART;VALUE=DATE:20261006
DTEND;VALUE=DATE:20261007
END:VEVENT
BEGIN:VEVENT
UID:optional
SUMMARY:Marked free (does not block)
TRANSP:TRANSPARENT
DTSTART;TZID=Europe/Vienna:20261008T140000
DTEND;TZID=Europe/Vienna:20261008T160000
END:VEVENT
END:VCALENDAR
"""


def cmd_selftest(_dav, args) -> None:
    start = datetime(2026, 10, 5, 7, 0).astimezone()  # a Monday morning
    end = start + timedelta(days=7)
    busy = busy_from_ics([SAMPLE], start, end, "sample")
    print("busy:")
    for b in busy:
        print(f"  {fmt(b.start)} - {fmt(b.end)}  {b.title}")
    # week Mon 5.10 07:00 .. Mon 12.10 07:00: lecture Mon 5 (Wed 7 excluded,
    # Mon 12 starts after the range), all-day Tue 6; the "free" event is skipped
    assert [(b.title[:7], b.start.day) for b in busy] == [("Lecture", 5), ("All-day", 6)], busy

    rules = rules_from(args)
    slots = free_slots(busy, start, 7, rules)
    print("\nfree:")
    show_free(slots)
    days = {a.day for a, _ in slots}
    assert 6 not in days, "all-day Tue must block the whole day"
    assert not days & {10, 11}, "weekend excluded by default"
    assert any(a.day == 7 and a.hour == 8 for a, _ in slots), "Wed lecture was excluded -> free from 08:00"
    mon = [(a, b) for a, b in slots if a.day == 5]
    assert mon[0][1].strftime("%H:%M") == "09:45" and mon[1][0].strftime("%H:%M") == "12:15", mon  # 15m buffer
    print("\nselftest ok")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name in ["list", "busy", "free", "write-test", "cleanup", "selftest"]:
        p = sub.add_parser(name)
        if name in ("busy", "free", "write-test"):
            p.add_argument("-c", "--calendars", required=True, help="comma-separated names")
        if name in ("busy", "free"):
            p.add_argument("--days", type=int, default=7)
        if name in ("free", "selftest"):
            p.add_argument("--day-start", default="08:00")
            p.add_argument("--day-end", default="22:00")
            p.add_argument("--weekends", action="store_true", help="also plan on Sat/Sun")
            p.add_argument("--buffer", type=int, default=15, help="minutes around busy times")
            p.add_argument("--min", type=int, default=30, help="shortest useful free slot")
    args = ap.parse_args()
    cmds = {"list": cmd_list, "busy": cmd_busy, "free": cmd_free, "write-test": cmd_write_test,
            "cleanup": cmd_cleanup, "selftest": cmd_selftest}
    dav = None if args.cmd == "selftest" else Dav(*credentials())
    cmds[args.cmd](dav, args)


if __name__ == "__main__":
    main()
