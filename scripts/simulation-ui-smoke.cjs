// UI fixtures only; no real campaign or Gemini call is triggered.
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const fs=require('node:fs/promises'),path=require('node:path'),assert=require('node:assert/strict');
const root=path.resolve(__dirname,'..'),base=process.env.MAGIC_DECK_TEST_URL||'http://127.0.0.1:18092';
const deck={id:'demo',name:'Démonstration · Les ailes de l’aube',format:'Historic',cardCount:60,mainDeck:[{name:'Plains',quantity:60,arenaId:1,typeLine:'Basic Land',colors:[]}],sideboard:[],commandZone:[],colors:['W'],wins:0,losses:0,events:[]};
const opponent={...deck,id:'reference-red',name:'Référence · Aggro rouge'};
const finished={id:'123',createdAt:1789287000,status:'completed',deck,opponent,gamesRequested:100,seed:42,secondsPerGame:120,engineVersion:'Fixture UI · données synthétiques',results:Array.from({length:100},(_,i)=>({index:i+1,outcome:i===99?'draw':i%3?'win':'loss',durationMs:2300,turns:9,seat:i%2+1})),error:null};
(async()=>{
  const browser=await chromium.launch({headless:true});
  await fs.mkdir(path.join(root,'.impeccable/review'),{recursive:true});
  try {
    for(const [name,viewport] of [['desktop',{width:1440,height:1000}],['mobile',{width:390,height:844}]]) {
      const context=await browser.newContext({viewport,reducedMotion:'reduce'});
      let ready=false,current=null,reads=0,launches=0;
      const engine=()=>({ready,settings:{forgeDir:ready?'/opt/forge':'',javaPath:'java'},problem:ready?null:'Installez Forge puis renseignez son dossier.',activeId:current?.status==='running'?'123':null});
      // Serve the current source while a separate real Forge batch may still be using an older binary.
      await context.route(base+'/',async route=>route.fulfill({contentType:'text/html',body:await fs.readFile(path.join(root,'web/index.html'),'utf8')}));
      await context.route(base+'/assets/*',async route=>{
        const name=new URL(route.request().url()).pathname.split('/').pop();
        if(!['app.js','app.css','coach.js','collection.js','preview.js','simulation.js','simulation.css'].includes(name)) return route.continue();
        await route.fulfill({contentType:name.endsWith('.css')?'text/css':'text/javascript',body:await fs.readFile(path.join(root,'web',name),'utf8')});
      });
      await context.route(base+'/api/**',route=>{
        const url=new URL(route.request().url()),method=route.request().method();let data={};
        if(url.pathname==='/api/status') data={deckCount:1,cardsOwned:0,totalCopies:0,wildcards:{rare:0,mythic:0},warnings:[],logExists:true};
        else if(url.pathname==='/api/decks') data=[deck];
        else if(url.pathname==='/api/collection') data=[];
        else if(url.pathname==='/api/simulation-opponents') data=[opponent];
        else if(url.pathname==='/api/simulation-engine') {if(method==='PUT') ready=true;data=engine();}
        else if(url.pathname==='/api/simulations'&&method==='POST') {
          const body=route.request().postDataJSON();assert.equal(body.games,100);assert.equal(body.deckId,'demo');
          launches++;reads=0;current={...finished,status:'running',results:[]};data=current;
        } else if(url.pathname==='/api/simulations') data=current?[{id:'123',createdAt:finished.createdAt,deckName:deck.name,opponentName:opponent.name,status:current.status,completed:current.results.length,gamesRequested:100,winRate:current.results.length?2/3:null}]:[];
        else if(url.pathname.endsWith('/cancel')) {current={...current,status:'cancelled'};data={ok:true};}
        else if(url.pathname==='/api/simulations/123') {if(launches===1&&++reads>1) current=finished;data=current;}
        return route.fulfill({json:data});
      });
      const page=await context.newPage(),errors=[];page.on('pageerror',e=>errors.push(e.message));
      await page.goto(base);await page.locator('[data-nav=simulation]').click();
      await page.locator('#sim-form').waitFor();assert.equal(await page.locator('#sim-start').isDisabled(),true);
      await page.locator('#forge-dir').fill('/opt/forge');await page.getByRole('button',{name:'Enregistrer et vérifier'}).click();
      await page.waitForFunction(()=>!document.getElementById('sim-start').disabled);
      await page.locator('#sim-engine-config summary').click();
      await page.locator('#sim-start').click();await page.waitForFunction(()=>document.querySelectorAll('.sim-game').length===100);
      assert.equal(await page.locator('.sim-game.win').count(),66);assert.equal(await page.locator('.sim-game.draw').count(),1);
      await page.locator('.sim-game').first().click();assert.match(await page.locator('#sim-game-detail').textContent(),/Partie 1 : Défaite/);
      await page.locator('.sim-protocol summary').click();assert.match(await page.locator('.sim-snapshot pre').first().textContent(),/60 Plains/);
      await page.locator('.sim-protocol summary').click();
      const download=page.waitForEvent('download');await page.getByRole('button',{name:'Télécharger le rapport JSON'}).click();assert.match((await download).suggestedFilename(),/123.json/);
      await page.evaluate(()=>scrollTo(0,0));await page.screenshot({path:path.join(root,`.impeccable/review/simulation-${name}.png`),fullPage:true});
      assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'No horizontal page overflow');
      await page.locator('#sim-start').click();
      await page.getByRole('button',{name:'Annuler la série'}).click();
      await page.getByText('Série annulée',{exact:true}).waitFor();
      await page.locator('[data-nav=collection]').click();await page.locator('[data-nav=simulation]').click();
      await page.getByText('Série annulée',{exact:true}).waitFor();
      assert.deepEqual(errors,[]);console.log(name+': configuration, 100 results, details, export, cancellation, history, layout OK');
      await context.close();
    }
  } finally {await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
