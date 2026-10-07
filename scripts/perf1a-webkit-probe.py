#!/usr/bin/env python3
"""Optional probe using installed GTK3/WebKitGTK 4.1 (no dependency installation).
Baseline: real frontend without Rust. Lifecycle: real frontend + synthetic IPC.
Focus and visibility are reported by the document, not inferred from GTK.show().
The process tree includes Python/WebKit; it is not the Tauri application's RSS.
"""
import argparse
import json
import os
from pathlib import Path
import sys
import time
sys.dont_write_bytecode = True

import importlib.util
sampler_spec = importlib.util.spec_from_file_location('process_baseline', Path(__file__).with_name('perf1a-process-baseline.py'))
sampler = importlib.util.module_from_spec(sampler_spec)
sampler_spec.loader.exec_module(sampler)
read_tree = sampler.read_tree

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--lifecycle', action='store_true')
parser.add_argument('--boot-contract', action='store_true', help='Assert fresh DEV Economy/Headless startup has no 3D resources')
parser.add_argument('--url', default='http://127.0.0.1:5173/')
args = parser.parse_args()
if args.lifecycle and args.url == 'http://127.0.0.1:5173/':
    args.url += '?presentation=presence'
if args.lifecycle and args.boot_contract:
    parser.error('Choose one probe scenario')
os.environ.setdefault('LIBGL_ALWAYS_SOFTWARE', '1')
import gi
gi.require_version('Gtk', '3.0')
gi.require_version('WebKit2', '4.1')
from gi.repository import Gtk, WebKit2, GLib

passed = False
manager = WebKit2.UserContentManager()
manager.register_script_message_handler('perf')

def message(manager, result):
    global passed
    text = result.get_js_value().to_string()
    print(text, flush=True)
    value = json.loads(text)
    if value.get('type') == 'lifecycle-result':
        passed = value.get('pass', False)
        Gtk.main_quit()

manager.connect('script-message-received::perf', message)
script = """for (const level of ['info','error','warn']) {
  const original=console[level]; console[level]=(...args)=>{
    original(...args);window.webkit.messageHandlers.perf.postMessage(JSON.stringify({type:'console',level,args}));
  };
}"""
if args.lifecycle or args.boot_contract:
    script += Path(__file__).with_name('fixtures').joinpath('perf1a-webkit-interaction.js').read_text()
manager.add_script(WebKit2.UserScript.new(script, WebKit2.UserContentInjectedFrames.TOP_FRAME,
                                         WebKit2.UserScriptInjectionTime.START, None, None))
view = WebKit2.WebView.new_with_user_content_manager(manager)
window = Gtk.Window()
window.set_default_size(310, 410)
window.add(view)
window.show_all()

def loaded(view, event):
    if event != WebKit2.LoadEvent.FINISHED:
        return
    code = Path(__file__).with_name('fixtures').joinpath('perf1a-webkit-cycles.js').read_text()
    if args.boot_contract:
        code = r"""(async () => {
          for(let i=0; i<100 && !window.__narysPerf1A; i++) await new Promise(r=>setTimeout(r,50));
          await new Promise(r=>setTimeout(r,1000));
          const state=window.__narysPerf1A.snapshot();
          const resources=performance.getEntriesByType('resource').map(e=>e.name).filter(n=>/avatar|three|Luna\.glb/i.test(n));
          const pass=state.presentation.phase==='detached' && state.presentation.mode!=='presence' && state.canvases===0 && resources.length===0;
          window.webkit.messageHandlers.perf.postMessage(JSON.stringify({type:'lifecycle-result',pass,state,resources,metrics:window.__fixture.metrics()}));
        })().catch(e=>window.webkit.messageHandlers.perf.postMessage(JSON.stringify({type:'lifecycle-result',pass:false,error:String(e)})));"""
    view.evaluate_javascript(code, -1, None, None, None, None)

if args.lifecycle or args.boot_contract:
    view.connect('load-changed', loaded)
view.load_uri(args.url)
started = time.monotonic()
samples = []
print(json.dumps(dict(type='environment', pid=os.getpid(), url=args.url,
                      softwareRendering=os.environ['LIBGL_ALWAYS_SOFTWARE'], session=os.getenv('XDG_SESSION_TYPE'),
                      webkit=[WebKit2.get_major_version(), WebKit2.get_minor_version(), WebKit2.get_micro_version()])), flush=True)

def finish_snapshot(view, result):
    print(view.evaluate_javascript_finish(result).to_string(), flush=True)
    Gtk.main_quit()

def sample():
    elapsed = time.monotonic() - started
    if elapsed >= 10:
        rows = read_tree(os.getpid())
        samples.append((time.monotonic(), rows))
        print(json.dumps(dict(type='processes', elapsed=elapsed, rows=rows)), flush=True)
    if elapsed < (180 if args.lifecycle or args.boot_contract else 40):
        return True
    cpu_ticks = 0
    churn = set()
    for previous, current in zip(samples, samples[1:]):
        old = {(row['pid'], row['startTicks']): row for row in previous[1]}
        new = {(row['pid'], row['startTicks']): row for row in current[1]}
        churn |= old.keys() ^ new.keys()
        cpu_ticks += sum(max(0, row['cpuTicks'] - old[key]['cpuTicks']) for key, row in new.items() if key in old)
    duration = samples[-1][0] - samples[0][0]
    print(json.dumps(dict(type='summary', duration=duration,
                          cpuPercentOneCore=100 * cpu_ticks / os.sysconf('SC_CLK_TCK') / duration,
                          rssKiB=sum(row['rssKiB'] for row in samples[-1][1]),
                          processCount=len(samples[-1][1]), processChurn=sorted(churn))), flush=True)
    view.evaluate_javascript(r"""JSON.stringify({
      type:'surface-snapshot', canvas:document.querySelectorAll('canvas').length,
      visibility:document.visibilityState, focus:document.hasFocus(),
      assets:performance.getEntriesByType('resource').filter(e=>e.name.endsWith('.glb')).map(e=>({name:e.name,duration:e.duration}))
    })""", -1, None, None, None, finish_snapshot)
    return False

GLib.timeout_add_seconds(1, sample)
Gtk.main()
if (args.lifecycle or args.boot_contract) and not passed:
    sys.exit(1)
