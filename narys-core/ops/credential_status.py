#!/usr/bin/python3
"""Legacy metadata-only wrapper; production embeds this adapter in the binary."""
from credential_manager import main
import sys
sys.argv = [sys.argv[0], 'status']
if __name__ == '__main__':
    try: main()
    except BaseException:
        print('{"service_available":false,"login_unlocked":false,"state":"unavailable","code":"credential_service_unavailable"}')
        raise SystemExit(1)
