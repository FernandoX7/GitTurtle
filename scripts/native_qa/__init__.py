"""Native-QA tooling for driving GitTurtle under XWayland; see README.md.

Modules that need Pillow, python-xlib, PyGObject or dbus-python import them
lazily, so `qa.py --help` and the unit tests run on a host without them.
"""
