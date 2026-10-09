;(async()=>{
 const wait=ms=>new Promise(r=>setTimeout(r,ms));const assert=(v,m)=>{if(!v)throw Error(m)}
 const until=async(fn,m)=>{let start=performance.now();while(!fn()){if(performance.now()-start>15000)throw Error(m);await wait(40)}}
 await until(()=>document.querySelector('.economy-shell')&&!document.querySelector('.nav-collapse').disabled,'bootstrap')
 document.querySelector('[aria-label="Recolher navegação"]')?.click()
 document.querySelector('[aria-label="Recolher painel operacional"]')?.click()
 document.querySelector('[aria-label="Terminal"]').click()
 await until(()=>document.querySelector('[data-terminal-connection=conectado]'),'activity connected')
 if(innerWidth<=740)[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Activity').click()
 window.__lr9dActivity()
 await until(()=>document.querySelectorAll('[data-trace-row]').length===window.__lr9dEvents.length,'adapter DTO rows')
 const rows=[...document.querySelectorAll('[data-trace-row]')].map(r=>r.innerText)
 for(const source of ['core/core','scheduler/scheduler','cognitive_provider/p','task_graph/task_graph','worker/worker','specialist_agent/codex'])assert(rows.some(r=>r.includes(source)),`missing provenance ${source}`)
 const text=rows.join('\n');assert(text.includes('task_started')&&text.includes('provider_selected')&&text.includes('subtask_started')&&text.includes('agent_completed'),'lifecycle codes')
 assert(text.includes('Mensagem natural')&&text.includes('Resumo explicitamente'),'agent display text')
 assert(!text.includes('SECRET')&&!text.includes('/private/raw'),'hygiene')
 window.webkit.messageHandlers.perf.postMessage(JSON.stringify({type:'shell-result',pass:true,gate:'LR-9D Activity real WebKit / native adapter DTO fixtures',rows,width:innerWidth,providerCalls:0,agentRequests:0}))
})().catch(e=>window.webkit.messageHandlers.perf.postMessage(JSON.stringify({type:'shell-result',pass:false,error:String(e)})))
