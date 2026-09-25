# CalDAV prototype

Throwaway Python probe for the timetable plan in `TODO.md` (section 3):
talk to iCloud via CalDAV the way udo would, before building it in Rust.
Raw HTTP + XML on purpose, so each step maps to the Rust version.

## Setup

```sh
cd prototypes/caldav
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
.venv/bin/python caldav_probe.py selftest     # offline, no account needed
```

## Credentials

Create an app-specific password at <https://account.apple.com> (Sign-In and
Security -> App-Specific Passwords). Then either:

```sh
export ICLOUD_USER=you@icloud.com
export ICLOUD_APP_PASSWORD=xxxx-xxxx-xxxx-xxxx
```

or store it in the macOS Keychain once (the script finds it there):

```sh
security add-generic-password -a you@icloud.com -s udo-icloud -w
```

Nothing is written to files except `.written.json` (hrefs of test blocks).

## Try it

```sh
.venv/bin/python caldav_probe.py list                          # calendars + subscriptions
.venv/bin/python caldav_probe.py busy -c "Uni,Personal"        # next 7 days
.venv/bin/python caldav_probe.py free -c "Uni,Personal" --buffer 15 --min 30
.venv/bin/python caldav_probe.py write-test -c udo             # asks first
.venv/bin/python caldav_probe.py cleanup                       # removes test blocks
```

Calendar names are the ones `list` prints. `write-test` needs a calendar
called e.g. `udo` that you created yourself in Apple Calendar.

## What to check

- Does `list` show the uni timetable as `subscription` with its ICS link?
  (Only if the subscription is stored in iCloud, not "On My Mac".)
- Do weekly lectures show up correctly in `busy`, including holidays /
  cancelled dates?
- Does the test block from `write-test` appear on the phone?
