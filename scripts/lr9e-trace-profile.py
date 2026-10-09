#!/usr/bin/env python3
"""Causal cfg(test) probe of the current bus using std/rustc only.
Instruments a temporary copy, never the production bus. Nested retention metrics
are inclusive; lock wait aggregates producer time, not wall time. No per-event log.
"""
import argparse,json,subprocess,tempfile
from pathlib import Path
parser=argparse.ArgumentParser();parser.add_argument('--release',action='store_true');parser.add_argument('--output',required=True);args=parser.parse_args()
root=Path(__file__).resolve().parents[1];bus=(root/'src-tauri/src/operational_trace/bus.rs').read_text()
bus=bus.replace('#[cfg(test)]\n#[path = "invariants.rs"]\nmod invariants;', '')
def replace(a,b):
 global bus
 assert a in bus,a
 bus=bus.replace(a,b,1)
replace('        let mut store = self.0.lock().unwrap_or_else(|p| p.into_inner());\n        let sequence', '        let timer = std::time::Instant::now();\n        let mut store = self.0.lock().unwrap_or_else(|p| p.into_inner());\n        probe(0, timer);\n        let sequence')
replace('        let timestamp = SystemTime::now()', '        let timer = std::time::Instant::now();\n        let timestamp = SystemTime::now()')
replace('        let event = Arc::new(OperationalEvent::observed(sequence, timestamp, draft));','        probe(1, timer);\n        let timer = std::time::Instant::now();\n        let event = Arc::new(OperationalEvent::observed(sequence, timestamp, draft));\n        probe(2, timer);')
replace('        let retained = store.retain_event(event.clone());','        let timer = std::time::Instant::now();\n        let retained = store.retain_event(event.clone());\n        probe(3, timer);\n        let timer = std::time::Instant::now();')
replace('        store.stats.live_delivery_dropped =','        probe(8, timer);\n        store.stats.live_delivery_dropped =')
replace('        let bytes = event.estimated_bytes();','        let timer = std::time::Instant::now();\n        let bytes = event.estimated_bytes();\n        probe(7, timer);\n        let timer = std::time::Instant::now();')
replace('        counts[class as usize] += 1;', '        probe(4, timer);\n        counts[class as usize] += 1;')
replace('            let victim = [','            let timer = std::time::Instant::now();\n            let victim = [')
replace('            let Some(index) = victim else {','            probe(5, timer);\n            let Some(index) = victim else {')
replace('            if let Some(victim) = self.events.remove(index) {','            let timer = std::time::Instant::now();\n            if let Some(victim) = self.events.remove(index) {')
replace('        self.stats.retained_bytes += bytes;', '            probe_unused();\n        self.stats.retained_bytes += bytes;') # marker for safe insertion below
replace('        }\n            probe_unused();','            probe(6, timer);\n        }')
bus+='''
#[cfg(test)] static TIMES: OnceLock<[AtomicU64;9]> = OnceLock::new();
#[cfg(test)] fn probe(index:usize, started:std::time::Instant) { TIMES.get_or_init(|| std::array::from_fn(|_|AtomicU64::new(0)))[index].fetch_add(started.elapsed().as_nanos() as u64,Ordering::Relaxed); }
#[test] fn causal_profile() {
 for producers in [1,10] {
  for metric in TIMES.get_or_init(|| std::array::from_fn(|_|AtomicU64::new(0))) {metric.store(0,Ordering::Relaxed);}
  let bus=OperationalTraceBus::isolated(); let slow=bus.subscribe().unwrap(); let started=std::time::Instant::now();
  std::thread::scope(|scope| {for n in 0..producers {let bus=bus.clone();scope.spawn(move||{for i in (n..12090).step_by(producers) {let mut draft=super::tests::stream("probe",&"x".repeat(128)); if i%200==0 {draft=EventDraft::new(draft.provenance.clone(),OperationalKind::Critical{kind:CriticalKind::Completed,code:TraceId::new("complete").unwrap(),message:TraceText::new("").unwrap()}).unwrap();} else if i%200==1 {draft=EventDraft::new(draft.provenance.clone(),OperationalKind::State{kind:StateKind::Started,code:TraceId::new("start").unwrap(),detail:TraceText::new("").unwrap()}).unwrap();} bus.publish(draft).unwrap();}});}});
  println!("PROFILE producers={} wall_ns={} nanoseconds={:?}",producers,started.elapsed().as_nanos(),TIMES.get().unwrap().each_ref().map(|m|m.load(Ordering::Relaxed)));drop(slow);
 }
 let draft=super::tests::stream("probe",&"x".repeat(128));let started=std::time::Instant::now();for _ in 0..1000000 {std::hint::black_box(std::hint::black_box(&draft).estimated_bytes());}println!("ESTIMATED_BYTES million_ns={}",started.elapsed().as_nanos());
}
'''
# Private fields are unavailable across modules: helper builds class before draft.
bus=bus.replace('draft.provenance.clone()','super::tests::provenance()')
with tempfile.TemporaryDirectory(prefix='lr9e-causal-') as temp:
 d=Path(temp);(d/'bus.rs').write_text(bus)
 harness='''mod luna {pub mod task {#[derive(Clone,Copy,Debug,Eq,PartialEq)] pub struct TaskId(pub u64);}}
mod operational_trace {
#[path="CONTRACT"] mod contract; pub use contract::*;
#[path="BUS"] mod bus;
pub mod tests {use super::*;pub fn provenance()->Provenance {Provenance{source:TraceSource{source_type:SourceType::Worker,id:TraceId::new("probe").unwrap(),instance:None},task_id:None,subtask_id:None,correlation_id:None,coalescing_key:None}} pub fn stream(_: &str,text:&str)->EventDraft {EventDraft::new(provenance(),OperationalKind::TextDelta{channel:TextChannel::ProviderText,text:TraceText::new(text).unwrap()}).unwrap()}}
}'''.replace('CONTRACT',str(root/'src-tauri/src/operational_trace/contract.rs')).replace('BUS',str(d/'bus.rs'))
 (d/'main.rs').write_text(harness);compile=subprocess.run(['rustc','--edition=2021','--test',str(d/'main.rs'),'-o',str(d/'probe'),'-C','opt-level='+('3' if args.release else '0')],capture_output=True,text=True);assert compile.returncode==0,compile.stderr
 run=subprocess.run([str(d/'probe'),'causal_profile','--nocapture'],capture_output=True,text=True);assert run.returncode==0,run.stderr
 import re
 names=['lock_wait','timestamp','event_creation','retain_inclusive','recount_or_counter_snapshot','victim_search','victim_removal','incoming_estimated_bytes','observers_try_send']
 result={'profile':'release' if args.release else 'debug','source':'temporary cfg(test) copy of current bus','categoriesInclusive':['retain_inclusive'],'runs':[]}
 for line in run.stdout.splitlines():
  if line.startswith('PROFILE'):
   m=re.match(r'PROFILE producers=(\d+) wall_ns=(\d+) nanoseconds=(.*)',line);result['runs'].append({'producers':int(m[1]),'wallNs':int(m[2]),'ns':dict(zip(names,json.loads(m[3])))})
  elif line.startswith('ESTIMATED_BYTES'):result['estimatedBytesOneMillionNs']=int(line.split('=')[1])
 Path(args.output).write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
