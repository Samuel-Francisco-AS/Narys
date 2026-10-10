#!/usr/bin/python3
"""Status only, no activation/prompt/item enumeration/unlock."""
import json, sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]/'experiments/lr-10a-sdk-runtime'))
from h2_manual_unlock import ExistingLogin
try:
    c=ExistingLogin()
    try: print(json.dumps({'service_available':True,'login_unlocked': c.locked() is False}))
    finally: c.close()
except BaseException:
    print(json.dumps({'service_available':False,'login_unlocked':False}))
    raise SystemExit(1)
