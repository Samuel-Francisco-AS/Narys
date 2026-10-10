#!/usr/bin/python3
"""A9-FIX-4R fixed one-shot identity; status/auth only, no flags or retry path."""
import sys
from confirm_a9_metadata import main

if __name__ == '__main__':
    if sys.argv[1:]:
        raise SystemExit(2)
    raise SystemExit(main('A9-FIX-4R'))
