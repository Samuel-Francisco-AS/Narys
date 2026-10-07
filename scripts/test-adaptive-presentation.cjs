// Real frontend guard hook, controlled IPC and effect clock; no 30-second waits.
const assert=require('node:assert/strict'),fs=require('node:fs'),ts=require('typescript'),Module=require('node:module')
const oldLoad=Module._load,oldTs=require.extensions['.ts']
let slots=[],index=0,effects=[],calls=[],listeners=new Map(),closing=false,resolveConfirmation
const flush=async()=>{for(let i=0;i<20;i++)await Promise.resolve()}
global.document={body:{inert:false}}
Module._load=function(id,...args){
 if(id==='react')return {
  useRef:value=>{const i=index++;return slots[i]??=( {current:value})},
  useEffect:(fn,deps)=>{const i=index++,old=slots[i];if(!old||deps.some((d,j)=>d!==old.deps[j])){slots[i]={deps,cleanup:old?.cleanup};effects.push(()=>{old?.cleanup?.();slots[i].cleanup=fn()})}}
 }
 if(id==='@tauri-apps/api/event')return {listen:async(name,fn)=>{listeners.set(name,fn);return()=>listeners.delete(name)}}
 if(id==='@tauri-apps/api/core')return {invoke:async(name,args)=>{calls.push({name,args});if(name==='confirm_auto_close'){return await new Promise(r=>{resolveConfirmation=()=>r(closing)})}}}
 return oldLoad.call(this,id,...args)
}
require.extensions['.ts']=(mod,file)=>mod._compile(ts.transpileModule(fs.readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,file)
;(async()=>{try{
 const {useAdaptiveGuard}=require('../src/presentation/useAdaptiveGuard.ts')
 let draft='';let snapshot={epoch:1,revision:1,state:'economy',policy:'auto',pendingToken:null,transitioning:false,attention:null}
 const render=()=>{index=0;useAdaptiveGuard(snapshot,snapshot.state,()=>draft.length>0);effects.splice(0).forEach(f=>f())}
 render();await flush();assert.equal(listeners.size,1)
 assert.equal(calls.filter(c=>c.name==='report_presentation_ui').length,1)
 draft='a';render();await flush()
 for(let i=0;i<100;i++){draft+='b';render()}
 await flush();assert.equal(calls.filter(c=>c.name==='report_presentation_ui').length,2,'keystrokes sent IPC')
 assert(calls.at(-1).args.guarded)
 // Final check observes the synchronous current draft, even before its next render.
 draft='';snapshot={...snapshot,pendingToken:7};render();await flush()
 draft='unsaved-before-render';listeners.get('adaptive-close-check')({payload:7});await flush()
 assert(document.body.inert);assert(calls.at(-1).args.guarded);assert.equal(calls.at(-1).args.token,7)
 closing=false;resolveConfirmation();await flush();assert(!document.body.inert)
 assert(!JSON.stringify(calls).includes('unsaved-before-render'))
 draft='';snapshot={...snapshot,epoch:2,revision:3};render();await flush()
 listeners.get('adaptive-close-check')({payload:8});await flush();assert.equal(calls.at(-1).args.epoch,2)
 closing=true;resolveConfirmation();await flush();assert(document.body.inert)
 snapshot={...snapshot,pendingToken:null};render();await flush();assert(!document.body.inert,'activation must thaw UI')
 snapshot={...snapshot,pendingToken:9};render();await flush()
 listeners.get('adaptive-close-check')({payload:9});await flush();const oldConfirmation=resolveConfirmation
 snapshot={...snapshot,pendingToken:10};render();await flush()
 listeners.get('adaptive-close-check')({payload:10});await flush()
 closing=false;oldConfirmation();await flush();assert(document.body.inert,'stale confirmation released a newer freeze')
 resolveConfirmation();await flush();assert(!document.body.inert)
 for(const slot of slots)slot?.cleanup?.();assert.equal(listeners.size,0)
 const native=fs.readFileSync('src-tauri/src/adaptive.rs','utf8')
 assert(!native.includes('from_secs(1)'));assert(!native.includes('interval('));assert(native.includes('from_secs(30)'))
 const normal=fs.readdirSync('dist/assets').filter(f=>f.endsWith('.js')).map(f=>fs.readFileSync('dist/assets/'+f,'utf8')).join('\n')
 for(const marker of ['__narysPerf1D','expire_for_probe','NARYS_PERF1D'])assert(!normal.includes(marker))
 const main=JSON.parse(fs.readFileSync('src-tauri/capabilities/main-window.json'))
 for(const cmd of ['report-presentation-ui','confirm-auto-close','get-presentation-snapshot'])assert(main.permissions.includes('allow-'+cmd))
 for(const file of ['settings-ai','settings-general'])assert(!JSON.parse(fs.readFileSync('src-tauri/capabilities/'+file+'.json')).permissions.includes('allow-confirm-auto-close'))
 console.log('PERF-1D guard: state transitions only, latest draft, incarnation, input freeze/release, listener cleanup, privacy, capabilities and production boundary PASS.')
}finally{Module._load=oldLoad;require.extensions['.ts']=oldTs}})().catch(e=>{console.error(e);process.exitCode=1})
