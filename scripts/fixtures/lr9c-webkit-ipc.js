// Synthetic native boundary only for WebKit DOM/lazy/renderer tests. No process.
;(() => {
 const base=window.__TAURI_INTERNALS__.invoke;let session=null,attachment=null,attaches=0,opens=0,closes=0,detaches=0,cursor=0n,traceEnabled=true,traceEpoch=0n;const sent=[]
 window.__TAURI_INTERNALS__.invoke=async(cmd,args={},options)=>{
  if(cmd==='terminal_session_status')return session
  if(cmd==='open_human_terminal'){opens++;session={sessionId:'9007199254740993',shell:'fixture-sh',startingDirectory:'/fixture',state:'running',rows:24,cols:80,exitCode:null,reaped:false};return session}
  if(cmd==='attach_terminal_surface'){traceEnabled=true;traceEpoch=0n;attaches++;attachment={...args,id:String(attaches)};if(session)args.status.onmessage(session);return {attachmentId:attachment.id}}
  if(cmd==='detach_terminal_surface'){detaches++;if(attachment?.id===args.attachmentId)attachment=null;return null}
  if(cmd==='acknowledge_terminal_batch')return null
  if(cmd==='set_terminal_activity'){if(traceEnabled!==args.enabled){traceEnabled=args.enabled;traceEpoch++}return String(traceEpoch)}
  if(cmd==='send_terminal_input'){sent.push([...args]);return null}
  if(cmd==='resize_terminal'){if(session)session={...session,rows:args.rows,cols:args.cols};return null}
  if(cmd==='close_human_terminal'){closes++;session={...session,state:'cancelled',reaped:true};attachment?.status.onmessage(session);return true}
  return base(cmd,args,options)
 }
 window.__terminalFixture={
  metrics:()=>({opens,closes,attaches,detaches,sent,session,traceEnabled,traceEpoch:String(traceEpoch)}),
  output:(text,missing=0n)=>{const body=new TextEncoder().encode(text),buffer=new ArrayBuffer(24+body.length),header=new DataView(buffer);header.setBigUint64(0,++cursor,true);header.setBigUint64(8,missing,true);header.setBigUint64(16,missing*8192n,true);new Uint8Array(buffer,24).set(body);attachment.pty.onmessage(buffer)},
  trace:(after,count,missing='0')=>traceEnabled&&attachment.activity.onmessage({deliveryEpoch:String(traceEpoch),cursor:String(after+count),missingEvents:missing,liveDeliveryDropped:missing,replayComplete:missing==='0',events:Array.from({length:count},(_,j)=>{let i=after+j+1;return {sequence:String(i),lastSequence:String(i),fragments:1,observedAtUnixMs:1,lastObservedAtUnixMs:1,class:i%100===0?'CRITICAL':i%100===1?'STATE':'STREAM',sourceType:'worker',sourceId:`source-${i%3}`,sourceInstance:null,taskId:i%3,subtaskId:null,correlationId:null,kind:'text_delta',code:null,channel:'stdout',text:`synthetic exact ${i}`}})})
 }
})()
