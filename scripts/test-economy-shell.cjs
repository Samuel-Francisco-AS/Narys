// Lightweight checks of real modules and every static dependency in the production graph.
const assert = require('node:assert/strict')
const fs = require('node:fs'), path = require('node:path'), ts = require('typescript'), Module = require('node:module')
const windowCalls = []
const nativeWindow = Object.fromEntries(['setMinSize','setResizable','setBackgroundColor','setSize'].map(name=>[name,async (...args)=>windowCalls.push({name,args})]))
global.window = { innerWidth:1120,innerHeight:720 }
const oldLoad = Module._load, oldTs = require.extensions['.ts']
Module._load = function(id, ...args) {
  if (id === '@tauri-apps/api/core') return {isTauri:()=>true}
  if(id === '@tauri-apps/api/window') return { getCurrentWindow:()=>nativeWindow, LogicalSize:class {constructor(width,height){this.width=width;this.height=height}}, PhysicalSize:class {constructor(width,height){this.width=width;this.height=height}} }
  if (id === '@tauri-apps/api/event') return {}
  if (id === 'react') return {}
  return oldLoad.call(this, id, ...args)
}
require.extensions['.ts'] = (mod, filename) => mod._compile(ts.transpileModule(fs.readFileSync(filename,'utf8').replaceAll('import.meta.env.DEV','false'), {compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,filename)
;(async () => {
try {
  const { WindowController } = require('../src/window/WindowController.ts')
  const wc = new WindowController(()=>{})
  await wc.setPresentation('economy'); assert(windowCalls.some(c=>c.name==='setBackgroundColor'&&c.args[0]==='#11151e'))
  assert(!windowCalls.some(c=>c.name==='setSize'), 'redundant initial resize')
  const count = windowCalls.length; await wc.setLayout('composer'); assert.equal(windowCalls.length,count,'legacy layout resized Economy')
  window.innerWidth=950; window.innerHeight=660
  await wc.setPresentation('presence'); assert(windowCalls.some(c=>c.name==='setResizable'&&c.args[0]===false))
  await wc.setPresentation('economy'); assert.deepEqual([windowCalls.at(-1).args[0].width,windowCalls.at(-1).args[0].height], [950,660])
  wc.dispose()
  const { PresentationController } = require('../src/presentation/PresentationController.ts')
  const { clampLayout } = require('../src/shell/shellPreferences.ts')
  assert.equal(new PresentationController().getSnapshot().mode, 'economy')
  assert.deepEqual(clampLayout({leftOpen:true,leftWidth:9999,rightOpen:false,rightWidth:1}), {leftOpen:true,leftWidth:320,rightOpen:false,rightWidth:220})
  assert.deepEqual(clampLayout({leftOpen:false,leftWidth:NaN,rightOpen:true,rightWidth:Infinity}), {leftOpen:false,leftWidth:208,rightOpen:true,rightWidth:272})
  const dir = 'dist/assets', assets = fs.readdirSync(dir)
  const visited = new Set()
  const traverse = file => {
    if (visited.has(file)) return
    visited.add(file)
    const source = fs.readFileSync(path.join(dir,file),'utf8')
    for (const marker of ['WebGLRenderer','GLTFLoader','AnimationMixer','/models/Luna.glb','__narysPerf1A','__fixture','FIX-1 injected']) assert(!source.includes(marker), `${marker} in initial graph: ${file}`)
    const ast = ts.createSourceFile(file,source,ts.ScriptTarget.Latest,true,ts.ScriptKind.JS)
    for (const node of ast.statements) if ((ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) && node.moduleSpecifier) {
      const name = node.moduleSpecifier.text
      if(name.startsWith('./')) traverse(path.basename(name))
    }
  }
  // The bootstrap chooses App via dynamic import; include it and the default visible summary explicitly.
  for (const prefix of ['index-', 'App-', 'OperationalSummary-']) {
    const entry = assets.find(n=>n.startsWith(prefix)&&n.endsWith('.js')); assert(entry); traverse(entry)
  }
  for(const file of assets.filter(n=>n.endsWith('.js'))) assert(!fs.readFileSync(path.join(dir,file),'utf8').includes('__narysPerf1A'), 'DEV API in production')
  const config = JSON.parse(fs.readFileSync('src-tauri/tauri.conf.json'))
  assert(config.app.windows[0].resizable && config.app.windows[0].minWidth===640 && config.app.windows[0].minHeight===480)
  const settings = fs.readFileSync('src/settings/GeneralSettingsApp.tsx','utf8')
  assert(settings.includes('value="economy"') && settings.includes('value="presence"'))
  // PERF-1D adds policies without expanding the concrete React mode contract.
  assert(settings.includes('value="auto"') && settings.includes('value="headless"'))
  assert.throws(() => new PresentationController().setMode('auto'))
  assert.throws(() => new PresentationController().setMode('headless'))
  console.log(`PERF-1B: defaults, restored layout clamp, native sizing, manual options, static production graph (${visited.size} chunks), DEV stripping PASS.`)
} finally { Module._load=oldLoad; require.extensions['.ts']=oldTs }

})().catch(error=>{console.error(error);process.exitCode=1})
