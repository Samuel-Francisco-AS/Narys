#!/usr/bin/python3
"""Reuse FIX1 ownership unchanged; only one closed worker invocation."""
import json, os, sys, subprocess
from pathlib import Path
ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'experiments/lr-10a-sdk-runtime'))
from measure import measure
import metadata_policy
if len(sys.argv) != 4:
    raise SystemExit(2)
before=metadata_policy.observe(Path.home()/'.copilot/config.json')
if before['structural_check']!='PASS_METADATA_ACCESS':
    print(json.dumps({'cleanup_complete':True,'sdk_report':{'state':'blocked','error_code':'configuration_structural_precondition_failed'}}))
    raise SystemExit(1)
result, code = measure(Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3]), 240)
result['headless']=None  # FIX1 means UI-free execution, not GUI absence.
result['worker_requests_no_gui']=True
r=subprocess.run(['/usr/bin/ps','-C','gnome-shell','-o','pid='],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,timeout=3)
result['gui_process_absence_observed']=r.returncode==1
after=metadata_policy.observe(Path.home()/'.copilot/config.json')
result['config_before']=before
result['config_after']=after
result['config_writer_attribution']='INCONCLUSIVE'
if after['structural_check']!='PASS_METADATA_ACCESS':
    code=1; result['configuration_structural_error']=True
# Private local response; never journal raw model output or protocol.
print(json.dumps(result))
raise SystemExit(code)
