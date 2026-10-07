const assert=require('node:assert/strict'),fs=require('node:fs'),ts=require('typescript'),Module=require('node:module')
const oldLoad=Module._load,oldTs=require.extensions['.ts']
let effects=[],listeners=new Map(),resolveSnapshot,resolveChoice
const flush=async()=>{for(let i=0;i<20;i++)await Promise.resolve()}
Module._load=function(id,...args){
 if(id==='react')return {useState:v=>[v,()=>{}],useRef:v=>({current:v}),useEffect:fn=>effects.push(fn)}
 if(id==='@tauri-apps/api/event')return {listen:async(name,fn)=>{listeners.set(name,fn);return()=>listeners.delete(name)}}
 if(id==='@tauri-apps/api/core')return {invoke:async(name)=>{
  if(name==='get_presentation_snapshot')return new Promise(r=>resolveSnapshot=r)
  if(name==='get_shell_settings')return {presentationMode:'economy',layout:{leftOpen:true,leftWidth:208,rightOpen:true,rightWidth:272}}
  if(name==='update_presentation_mode')return new Promise(r=>resolveChoice=r)
  throw Error(name)
 }}
 return oldLoad.call(this,id,...args)
}
require.extensions['.ts']=(mod,file)=>mod._compile(ts.transpileModule(fs.readFileSync(file,'utf8').replaceAll('import.meta.env.DEV','false'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,file)
;(async()=>{try{
 const {PresentationController}=require('../src/presentation/PresentationController.ts')
 const {useShellPreferences}=require('../src/shell/shellPreferences.ts')
 const controller=new PresentationController(), seen=[];controller.subscribe(()=>seen.push(controller.getSnapshot().mode))
 const prefs=useShellPreferences(controller),cleanup=effects[0]();await flush()
 // This callback was created before native hydration. Its late result must not select 3D.
 const choice=prefs.chooseMode('presence');await flush()
 const auto={policy:'auto',state:'economy',epoch:1,revision:3,pendingToken:null,attention:null,transitioning:false}
 listeners.get('adaptive-presentation-changed')({payload:auto});await flush()
 resolveSnapshot({...auto,policy:'presence',state:'presence',revision:2});resolveChoice();await choice;await flush()
 assert.equal(controller.getSnapshot().mode,'economy');assert(!seen.includes('presence'),'stale opt-in loaded Presence after Auto')
 listeners.get('adaptive-presentation-changed')({payload:{...auto,policy:'headless',epoch:2,revision:4}});await flush()
 assert.equal(controller.getSnapshot().mode,'economy','manual Headless temporary control loaded Presence')
 cleanup();assert.equal(listeners.size,0)
 console.log('PERF-1D hydration: stale opt-in/result cannot overwrite Auto; manual Headless temporary Economy; listener cleanup PASS.')
}finally{Module._load=oldLoad;require.extensions['.ts']=oldTs}})().catch(e=>{console.error(e);process.exitCode=1})
