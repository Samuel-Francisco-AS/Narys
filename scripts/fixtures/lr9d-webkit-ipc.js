// Activity-only fixture. DTOs come from native LR-9D adapters, no inference/shell.
;(() => {
 const base=window.__TAURI_INTERNALS__.invoke;let activity=null
 window.__TAURI_INTERNALS__.invoke=async(cmd,args={})=>{
  if(cmd==='terminal_session_status')return null
  if(cmd==='attach_terminal_surface'){activity=args.activity;return {attachmentId:'lr9d-fixture'}}
  if(cmd==='detach_terminal_surface'){activity=null;return null}
  if(cmd==='acknowledge_terminal_batch')return null
  return base(cmd,args)
 }
 window.__lr9dActivity=()=>activity.onmessage({cursor:window.__lr9dEvents.at(-1).lastSequence,missingEvents:'0',liveDeliveryDropped:'0',replayComplete:true,events:window.__lr9dEvents})
})()
