// Requires the isolated server described in docs/SIMULATION.md and real Forge.
const assert=require('node:assert/strict');
const base=process.env.MAGIC_DECK_TEST_URL||'http://127.0.0.1:18092';
const request={deckId:'simulation-red',opponentId:'reference-green',games:2,seed:42,secondsPerGame:30};
async function call(path,body) {
  const response=await fetch(base+path,body?{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)}:{});
  const data=await response.json();assert.equal(response.ok,true,JSON.stringify(data));return data;
}
async function terminal(id) {
  for(let i=0;i<180;i++) {
    const r=await call('/api/simulations/'+id);
    if(!['starting','running'].includes(r.status)) return r;
    await new Promise(resolve=>setTimeout(resolve,1000));
  }
  throw new Error('Campaign did not terminate within test budget');
}
(async()=>{
  let id=process.env.SIMULATION_COMPLETED_ID;
  if(!id) id=(await call('/api/simulations',request)).id;
  const complete=await terminal(id);assert.equal(complete.status,'completed');assert.equal(complete.results.length,complete.gamesRequested);
  assert.ok(complete.engineVersion);assert.equal(complete.deck.mainDeck.reduce((n,c)=>n+c.quantity,0),60);
  assert.equal(new Set(complete.results.map(g=>g.index)).size,complete.gamesRequested);
  console.log(`Real Forge: ${complete.results.length} games; ${complete.results.filter(g=>g.outcome==='win').length} wins, ${complete.results.filter(g=>g.outcome==='loss').length} losses.`);
  const missing=await call('/api/simulations',{...request,deckId:'simulation-missing',games:1});
  const rejected=await terminal(missing.id);assert.equal(rejected.status,'failed');assert.equal(rejected.results.length,0);assert.match(rejected.error,/Cartes absentes/);
  const long=await call('/api/simulations',{...request,games:1000});
  const duplicate=await fetch(base+'/api/simulations',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(request)});
  assert.equal(duplicate.status,400);
  await call('/api/simulations/'+long.id+'/cancel',{});
  const cancelled=await terminal(long.id);assert.equal(cancelled.status,'cancelled');assert.ok(cancelled.results.length<1000);
  const history=await call('/api/simulations');assert.ok(history.some(r=>r.id===id));
  const invalid=await fetch(base+'/api/simulations/not-a-run');assert.equal(invalid.status,404);
  console.log('Exact snapshots, unsupported card rejection, concurrency limit, cancellation and history OK');
})().catch(error=>{console.error(error);process.exitCode=1;});
