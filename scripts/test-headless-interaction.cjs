// Deterministic checks of the real React hook and IPC client, independent of native gate.
const assert = require('node:assert/strict'), fs = require('node:fs'), ts = require('typescript'), Module = require('node:module')
const originalLoad = Module._load, originalTs = require.extensions['.ts']
let effects, published, calls, callback, task, messages
class Channel { constructor(fn) { this.onmessage=fn } }
Module._load = function(id,...args) {
  if(id==='react') return { useEffect:fn=>effects.push(fn), useRef:value=>({current:value}), useState:initial=>[initial,next=>{published=next}] }
  if(id==='@tauri-apps/api/core') return { Channel, isTauri:()=>true, invoke:async (command,args)=> {
    calls.push([command,args])
    if(command==='get_current_interaction') return {sessionId:42,task}
    if(command==='get_conversation_session') return {id:42,messages}
    if(command==='attach_conversation_events') {
      callback=args.channel.onmessage
      // Native broker replays before the attach result arrives. Old chunks must
      // not be fabricated as a complete streaming prefix after a truncated replay.
      callback({taskId:301,sequence:50,state:'running',type:'provider_chunk',chunk:'partial historical text'})
      return task
    }
    if(command==='cancel_task') return true
    throw Error(`unexpected IPC ${command}`)
  } }
  return originalLoad.call(this,id,...args)
}
require.extensions['.ts']=(mod,file)=>mod._compile(ts.transpileModule(fs.readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText,file)
const flush=async()=>{for(let i=0;i<20;i++) await Promise.resolve()}
;(async()=>{
  try {
    const {useConversationController}=require('../src/conversation/ConversationController.ts')
    effects=[];calls=[];messages=[{id:1,sessionId:42,role:'assistant',content:'persisted'}]
    task={taskId:301,sessionId:42,state:'running',sequence:60,terminal:null,replayComplete:false}
    const hook=useConversationController(), cleanup=effects[0]()
    await flush()
    assert.equal(published.sessionId,42); assert.equal(published.activeTaskId,301)
    assert.equal(published.preview,''); assert.equal(published.assistantStreaming,true)
    assert.match(published.providerRoute,/Resposta em andamento/)
    assert(!calls.some(([c])=>c==='start_conversation_task'))
    callback({taskId:302,sequence:61,state:'running',type:'provider_chunk',chunk:'other task'})
    assert.equal(published.preview,'')
    callback({taskId:301,sequence:61,state:'running',type:'provider_chunk',chunk:'future'})
    assert.equal(published.preview,'future')
    callback({taskId:301,sequence:61,state:'running',type:'provider_chunk',chunk:'duplicate'})
    assert.equal(published.preview,'future')
    hook.cancel(); await flush()
    assert.equal(calls.find(([c])=>c==='cancel_task')[1].taskId,301)
    const cancels=calls.filter(([c])=>c==='cancel_task').length
    cleanup(); assert.equal(calls.filter(([c])=>c==='cancel_task').length,cancels)
    callback({taskId:301,sequence:62,state:'cancelled',type:'task_cancelled'})
    assert.equal(published.activeTaskId,301,'stale callback after unmount changed Interaction')
    effects=[];calls=[];task={...task,state:'completed',sequence:62,terminal:{taskId:301,sequence:62,state:'completed',type:'task_completed'}}
    messages=[...messages,{id:2,sessionId:42,role:'assistant',content:'terminal persisted answer'}]
    useConversationController();effects[0]();await flush()
    assert.equal(published.activeTaskId,null);assert.equal(published.assistantStreaming,false)
    assert.equal(published.messages.at(-1).content,'terminal persisted answer')
    assert(!calls.some(([c])=>c==='start_conversation_task'))
    const capability=JSON.parse(fs.readFileSync('src-tauri/capabilities/main-window.json'))
    assert(capability.permissions.includes('allow-attach-conversation-events'))
    for(const file of ['settings-ai.json','settings-general.json']) assert(!JSON.parse(fs.readFileSync('src-tauri/capabilities/'+file)).permissions.includes('allow-attach-conversation-events'))
    console.log('PERF-1C Interaction: same TaskId/session, scoped future events, truncated replay, terminal SQLite reload, no restart, explicit cancel and no unmount cancellation PASS.')
  } finally {Module._load=originalLoad;require.extensions['.ts']=originalTs}
})().catch(error=>{console.error(error);process.exitCode=1})
