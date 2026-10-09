#!/usr/bin/env python3
"""Run the built LR-9D-compatible fixture repeatedly, excluding compilation.
Usage: --binary .../deps/assistente_3d_lib-... --output /tmp/...json
The caller builds debug/release first; this driver never builds or starts Vite.
"""
import argparse,hashlib,json,os,re,statistics,subprocess
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--output',required=True);p.add_argument('--runs',type=int,default=3);p.add_argument('--workload',choices=['both','extreme','representative'],default='both');p.add_argument('--profile',choices=['debug','release'],required=True);args=p.parse_args()
result={'profile':args.profile,'runs':args.runs,'binarySha256':hashlib.sha256(Path(args.binary).read_bytes()).hexdigest(),'compilationIncluded':False,'samples':[],'medians':[]}
for workload in (['extreme','representative'] if args.workload=='both' else [args.workload]):
 for n in range(args.runs):
  env={**os.environ};env.pop('NARYS_LR9E_REPRESENTATIVE',None)
  if workload=='representative':env['NARYS_LR9E_REPRESENTATIVE']='1'
  run=subprocess.run([args.binary,'multi_source_stress_and_reproducible_overhead_gate','--nocapture'],env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
  assert run.returncode==0,run.stdout[-4000:]
  samples=[json.loads(l.split(' ',1)[1]) for l in run.stdout.splitlines() if l.startswith('LR9E_TRACE ')]
  if not samples:
   assert workload=='extreme','legacy fixture cannot measure representative'
   for line in run.stdout.splitlines():
    if line.startswith('LR9D overhead'):
     d={k:int(v) for k,v in re.findall(r'(\w+)=(\d+)',line)}
     samples.append({'mode':d['mode'],'workload':workload,'sourceEvents':d['source_events'],'operationalEvents':d['operational_events'],'elapsedUs':d['elapsed_ms']*1000,'retainedEvents':d['retained'],'retainedBytes':d['bytes'],'evicted':{'stream':d['evicted_stream'],'state':0,'critical':0},'dropped':{'stream':d['dropped_stream'],'state':0,'critical':0},'liveDeliveryDrops':d['live_dropped'],'producers':10,'providerCalls':22,'agentTurnStarts':2,'legacyOutputPrecision':'milliseconds','inputTokens':77,'outputTokens':91})
  for sample in samples:
   sample.update({'run':n+1,'eventsPerSecond':round(sample['operationalEvents']*1000000/sample['elapsedUs'],2),'sourceEventsPerSecond':round(sample['sourceEvents']*1000000/sample['elapsedUs'],2),'usPerOperationalEvent':round(sample['elapsedUs']/sample['operationalEvents'],3) if sample['operationalEvents'] else None});result['samples'].append(sample)
  print(workload,n+1,[s['elapsedUs'] for s in samples],flush=True)
 for mode in range(3):
  samples=[s for s in result['samples'] if s['mode']==mode and s['workload']==workload];result['medians'].append({'workload':workload,'mode':mode,'elapsedUs':statistics.median(s['elapsedUs'] for s in samples),'eventsPerSecond':statistics.median(s['eventsPerSecond'] for s in samples),'sourceEventsPerSecond':statistics.median(s['sourceEventsPerSecond'] for s in samples),'usPerOperationalEvent':statistics.median(s['usPerOperationalEvent'] for s in samples) if mode else None})
Path(args.output).write_text(json.dumps(result,indent=2)+'\n')
