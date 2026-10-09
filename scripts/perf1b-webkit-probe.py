#!/usr/bin/env python3
"""Production frontend in real WebKitGTK, synthetic IPC only (not Tauri RSS).
Uses PERF-1A /proc methodology: 10s warmup + 30s sampling, percent of one core.
No DEV lifecycle API is needed. No provider or commercial call is performed.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import sys
import time
sys.dont_write_bytecode = True
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--url', default='http://127.0.0.1:4173/')
parser.add_argument('--mode', choices=['economy', 'presence'], default='economy')
parser.add_argument('--test', action='store_true')
parser.add_argument('--lr9d', action='store_true', help='LR-9D Activity native adapter DTO fixture')
parser.add_argument('--lr9c', action='store_true', help='LR-9C synthetic IPC/real xterm DOM suite')
parser.add_argument('--screenshot')
parser.add_argument('--width', type=int, default=1120)
parser.add_argument('--restored-layout', action='store_true')
parser.add_argument('--qa', action='store_true')
args = parser.parse_args()
os.environ.setdefault('LIBGL_ALWAYS_SOFTWARE', '1')
import gi
gi.require_version('Gtk', '3.0')
gi.require_version('WebKit2', '4.1')
from gi.repository import Gtk, WebKit2, GLib
spec = importlib.util.spec_from_file_location('sampler', Path(__file__).with_name('perf1a-process-baseline.py'))
sampler = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sampler)
manager = WebKit2.UserContentManager()
manager.register_script_message_handler('perf')
passed = False

def message(_, result):
    global passed
    text = result.get_js_value().to_string()
    print(text, flush=True)
    data = json.loads(text)
    if data.get('type') == 'shell-result':
        passed = data.get('pass', False)
        if args.lr9d and args.screenshot and passed:
            def save_activity(view, result):
                view.get_snapshot_finish(result).write_to_png(args.screenshot)
                Gtk.main_quit()
            view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE, WebKit2.SnapshotOptions.NONE, None, save_activity)
        else:
            Gtk.main_quit()
manager.connect('script-message-received::perf', message)
fixture = Path(__file__).with_name('fixtures').joinpath('perf1a-webkit-interaction.js').read_text()
fixture = fixture.replace("presentationMode: 'economy'", "presentationMode: '" + args.mode + "'")
if args.restored_layout:
    fixture = fixture.replace('leftWidth: 208', 'leftWidth: 9999').replace('rightWidth: 272', 'rightWidth: 1')
if args.lr9c:
    fixture += Path(__file__).with_name('fixtures').joinpath('lr9c-webkit-ipc.js').read_text()
if args.lr9d:
    activity = Path(__file__).resolve().parent.parent / 'docs' / 'LR-9D-ACTIVITY-FIXTURE.json'
    fixture += "\nwindow.__lr9dEvents=" + activity.read_text() + ";\n"
    fixture += Path(__file__).with_name('fixtures').joinpath('lr9d-webkit-ipc.js').read_text()
# Count actual context acquisition and delivered animation callbacks, not target FPS.
fixture += r"""
window.__graphics = { contexts: 0, frames: 0, frames3D: 0, drawCalls: 0, frameStamp: null, lastDrawStamp: null, bootstrapMs: null };
const contexts = new WeakSet(), originalContext = HTMLCanvasElement.prototype.getContext;
HTMLCanvasElement.prototype.getContext = function(type,...args) {
  const result = originalContext.call(this,type,...args);
  if(type.startsWith('webgl') && result && !contexts.has(result)) { contexts.add(result); window.__graphics.contexts++;
    for(const method of ['drawElements','drawArrays','drawElementsInstanced','drawArraysInstanced']) {
      if(!result[method]) continue;
      const draw = result[method].bind(result);
      result[method] = (...values) => {
        window.__graphics.drawCalls++;
        if(window.__graphics.lastDrawStamp !== window.__graphics.frameStamp) {
          window.__graphics.lastDrawStamp = window.__graphics.frameStamp; window.__graphics.frames3D++;
        }
        return draw(...values);
      };
    } }
  return result;
};
const originalRAF = window.requestAnimationFrame;
window.requestAnimationFrame = fn => originalRAF(at => { window.__graphics.frames++; window.__graphics.frameStamp = at; fn(at); });
const bootObserver = new MutationObserver(() => {
  const root = document.querySelector('[data-presentation-mode]');
  if(root && (root.dataset.presentationMode==='economy' && document.querySelector('.nav-collapse')?.disabled===false || root.dataset.presentationPhase==='ready')) {
    window.__graphics.bootstrapMs = performance.now(); bootObserver.disconnect();
  }
});
bootObserver.observe(document, { childList:true,subtree:true,attributes:true,attributeFilter:['data-presentation-phase','disabled'] });
"""
manager.add_script(WebKit2.UserScript.new(fixture, WebKit2.UserContentInjectedFrames.TOP_FRAME, WebKit2.UserScriptInjectionTime.START, None, None))
view = WebKit2.WebView.new_with_user_content_manager(manager)
window = Gtk.Window()
window.set_default_size(args.width, 720)
window.add(view)
window.show_all()

def loaded(view, event):
    if event == WebKit2.LoadEvent.FINISHED and args.test:
        code = Path(__file__).with_name('fixtures').joinpath('lr9d-webkit-activity.js' if args.lr9d else 'lr9c-webkit-terminal.js' if args.lr9c else 'perf1b-webkit-shell.js').read_text()
        view.evaluate_javascript(code, -1, None, None, None, None)
view.connect('load-changed', loaded)
view.load_uri(args.url)
started = time.monotonic()
samples = []
graphics_samples = []
print(json.dumps(dict(type='environment', productionFrontend=True, syntheticIPC=True, mode=args.mode, url=args.url, session=os.getenv('XDG_SESSION_TYPE'), softwareRendering=os.environ['LIBGL_ALWAYS_SOFTWARE'], webkit=[WebKit2.get_major_version(),WebKit2.get_minor_version(),WebKit2.get_micro_version()])), flush=True)

def snapshot_finished(view, result):
    print(view.evaluate_javascript_finish(result).to_string(), flush=True)
    Gtk.main_quit()

def image_finished(view, result):
    view.get_snapshot_finish(result).write_to_png(args.screenshot)
    if args.qa:
        Gtk.main_quit()

def graphics_sample(view, result):
    values = json.loads(view.evaluate_javascript_finish(result).to_string())
    graphics_samples.append(values)
    print(json.dumps(dict(type='graphics-sample', **values)),flush=True)

def sample():
    elapsed = time.monotonic() - started
    if elapsed >= 10 and not args.test:
        view.evaluate_javascript("JSON.stringify({atMs:performance.now(),...window.__graphics})", -1, None, None, None, graphics_sample)
        rows = sampler.read_tree(os.getpid())
        samples.append((time.monotonic(), rows))
        print(json.dumps(dict(type='sample', elapsedSeconds=elapsed, rssKiB=sum(r['rssKiB'] for r in rows), processes=rows)), flush=True)
    if elapsed < (100 if args.test else 40):
        return True
    if args.test:
        print(json.dumps({'type':'shell-result','pass':False,'error':'probe timeout'}),flush=True)
        Gtk.main_quit()
        return False
    ticks = 0
    churn = set()
    for prev, curr in zip(samples, samples[1:]):
        old = {(r['pid'],r['startTicks']):r for r in prev[1]}
        new = {(r['pid'],r['startTicks']):r for r in curr[1]}
        churn |= old.keys() ^ new.keys()
        ticks += sum(max(0,r['cpuTicks']-old[k]['cpuTicks']) for k,r in new.items() if k in old)
    duration = samples[-1][0] - samples[0][0]
    print(json.dumps(dict(type='summary',durationSeconds=duration,cpuPercentOneCore=100*ticks/os.sysconf('SC_CLK_TCK')/duration,rssMinKiB=min(sum(r['rssKiB'] for r in rows) for _,rows in samples),rssMaxKiB=max(sum(r['rssKiB'] for r in rows) for _,rows in samples),rssFinalKiB=sum(r['rssKiB'] for r in samples[-1][1]),processCount=len(samples[-1][1]),processChurn=sorted(churn))),flush=True)
    view.evaluate_javascript("JSON.stringify({type:'surface-snapshot',mode:document.querySelector('[data-presentation-mode]')?.dataset.presentationMode,canvases:document.querySelectorAll('canvas').length,graphics:window.__graphics,focus:document.hasFocus(),visibility:document.visibilityState,resources:performance.getEntriesByType('resource').map(e=>({name:e.name,duration:e.duration})),metrics:window.__fixture.metrics()})",-1,None,None,None,snapshot_finished)
    return False
def screenshot_ready(view, result):
    if view.evaluate_javascript_finish(result).to_boolean():
        GLib.timeout_add_seconds(1, lambda: (view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE, WebKit2.SnapshotOptions.NONE, None, image_finished), False)[1])
    else:
        GLib.timeout_add_seconds(1, check_screenshot)

def check_screenshot():
    view.evaluate_javascript("Boolean(document.querySelector('.economy-shell') && !document.querySelector('.nav-collapse')?.disabled)", -1, None, None, None, screenshot_ready)
    return False

if args.screenshot and not args.lr9d:
    GLib.timeout_add_seconds(1, check_screenshot)
GLib.timeout_add_seconds(1,sample)
Gtk.main()
if args.test and not passed:
    sys.exit(1)
