// Experiments have their own lifecycle and never modify Arena match statistics.
const simulation = { engine:null, references:[], reports:[], randomReports:[], selected:null, timer:null, randomLabTimer:null, randomLabId:null, generation:0, preferredDeck:null, selectedGame:null, labRunning:false, section:'duel', pendingSection:null };
const simStatuses = { starting:'Vérification des cartes', running:'Parties en cours', completed:'Série terminée', cancelled:'Série annulée', failed:'Série arrêtée sur erreur', interrupted:'Série interrompue' };
const simOutcome = { win:'Victoire', loss:'Défaite', draw:'Égalité', timeout:'Délai dépassé' };
const simActive = report => ['starting','running'].includes(report?.status);
const simPct = value => `${Math.round(value * 100)} %`;
function challengeDeck(id) { simulation.preferredDeck=id; simulation.section='duel'; showView('simulation'); }
function syncSimulationNavigation() {
  document.querySelectorAll('[data-simulation-nav]').forEach(button => {
    const active=button.dataset.simulationNav===simulation.section;
    button.classList.toggle('is-active',active);
    if(active) button.setAttribute('aria-current','page'); else button.removeAttribute('aria-current');
  });
  document.querySelectorAll('[data-sim-screen]').forEach(screen => {
    const active=screen.dataset.simScreen===simulation.section;
    screen.hidden=!active;
    screen.classList.toggle('is-active',active);
  });
}
function organizeSimulationScreens(container) {
  const screens=document.createElement('div');
  screens.className='simulation-screens';
  const groups=[
    ['duel',['#sim-form']],
    ['lab',['#sim-lab']],
    ['random',['#sim-random-lab']],
    ['reports',['#sim-result','#random-history','#sim-history']],
    ['engine',['.sim-engine-line','#sim-engine-config']],
  ];
  groups.forEach(([name,selectors])=>{
    const screen=document.createElement('section');
    screen.className='simulation-screen'; screen.dataset.simScreen=name;
    selectors.map(selector=>container.querySelector(selector)).filter(Boolean).forEach(element=>screen.appendChild(element));
    screens.appendChild(screen);
  });
  container.appendChild(screens);
  syncSimulationNavigation();
}
function simulationSectionTarget(section) {
  if(section==='lab') return document.getElementById('sim-lab');
  if(section==='random') return document.getElementById('sim-random-lab');
  if(section==='reports') return document.getElementById('sim-result')?.hasChildNodes()?document.getElementById('sim-result'):document.getElementById('sim-history');
  if(section==='engine') return document.getElementById('sim-engine-config');
  return document.getElementById('sim-form');
}
function focusSimulationSection(section, shouldScroll=true) {
  simulation.section=section;
  syncSimulationNavigation();
  const target=simulationSectionTarget(section);
  if(section==='engine' && target) target.open=true;
  if(shouldScroll && target) target.scrollIntoView({block:'start',behavior:reduceMotion?'auto':'smooth'});
}
function showSimulationSection(section) {
  simulation.pendingSection=section;
  simulation.section=section;
  syncSimulationNavigation();
  if(state.currentView!=='simulation') { showView('simulation'); return; }
  if(!simulationSectionTarget(section)) return;
  simulation.pendingSection=null;
  focusSimulationSection(section);
}
function openSimulationReport(id) { focusSimulationSection('reports',false); loadSimulation(id).then(()=>simulationSectionTarget('reports')?.scrollIntoView({block:'start',behavior:reduceMotion?'auto':'smooth'})); }
function simulationDeckOptions(decks, selected) {
  return decks.map(deck=>`<option value="${escapeHtml(deck.id)}" ${deck.id===selected?'selected':''} ${deck.commandZone.length || /brawl|commander|oathbreaker/i.test(deck.format||'')?'disabled':''}>${escapeHtml(deck.name)} · ${deck.cardCount} cartes</option>`).join('');
}
function standardDeckOptions(decks, selected) { return decks.filter(deck=>/standard/i.test(deck.format||'')).map(deck=>`<option value="${escapeHtml(deck.id)}" ${deck.id===selected?'selected':''}>${escapeHtml(deck.name)} · ${deck.cardCount} cartes</option>`).join(''); }
async function renderSimulation() {
  clearTimeout(simulation.timer);
  const generation=++simulation.generation, container=document.getElementById('view-simulation');
  container.innerHTML=heading('', 'Simulation', 'Votre deck et son adversaire sont pilotés par les IA de Forge.')+'<p role="status">Connexion au laboratoire…</p>';
  try {
    const [engine,references,reports,randomReports]=await Promise.all([api('/api/simulation-engine'),api('/api/simulation-opponents'),api('/api/simulations'),api('/api/random-lab')]);
    if(generation!==simulation.generation || state.currentView!=='simulation') return;
    Object.assign(simulation,{engine,references,reports,randomReports});
    container.innerHTML=`<div class="sim-hero"><div><span class="sim-kicker">Atelier de test · Forge</span><h1>Challenger un deck</h1><p>Transformez vos hypothèses en parties réelles. Comparez vos choix, observez les tendances et gardez une trace de chaque expérience.</p></div><div class="sim-engine-badge ${engine.ready?'is-ready':'is-missing'}"><span class="sim-engine-dot"></span><span id="sim-engine-state">${engine.ready?'Forge opérationnel':'Forge à configurer'}</span></div></div>
      <div class="sim-engine-line"><span class="sim-engine-hint">${engine.ready?'Le laboratoire est prêt pour votre prochaine série.':'Le moteur doit être configuré avant de lancer un duel.'}</span><button class="text-button" onclick="showSimulationSection('engine')">Configurer le moteur</button></div>
      <details id="sim-engine-config" class="sim-config" ${engine.ready?'':'open'}><summary>Installation et réglages de Forge</summary>
      <p>Installez la distribution desktop <a href="https://github.com/Card-Forge/forge/releases/tag/forge-2.0.14" target="_blank" rel="noopener noreferrer">Forge 2.0.14</a> dans un dossier dédié, avec un JDK 17 ou supérieur. Gardez le dossier <code>res</code> à côté du fichier JAR. Sous Ubuntu, le script <code>scripts/install-forge.sh</code> prépare cette installation.</p>
      <form onsubmit="saveSimulationEngine(event)" class="sim-settings-form"><label>Dossier de Forge<input id="forge-dir" required value="${escapeHtml(engine.settings.forgeDir)}" placeholder="/chemin/vers/forge-2.0.14"></label><label>Exécutable Java<input id="forge-java" required value="${escapeHtml(engine.settings.javaPath)}" placeholder="java"></label><button class="action-secondary" type="submit">Enregistrer et vérifier</button></form>
      <p id="sim-engine-feedback" role="status">${escapeHtml(engine.problem||'Le moteur est prêt. Les cartes seront vérifiées au lancement.')}</p></details>
      <form id="sim-form" class="sim-setup plate" onsubmit="startSimulation(event)"><div class="sim-section-heading"><div><span class="sim-kicker">Nouvelle expérience</span><h2>Préparer le duel</h2></div><span class="sim-bo1">BO1 · sans réserve</span></div>
      <div class="sim-duel"><div class="sim-deck-pick"><span class="sim-pick-label">Votre deck</span><label><select id="sim-deck" required>${simulationDeckOptions(state.decks,simulation.preferredDeck||state.currentDeckId)}</select></label><small>La liste que vous voulez éprouver</small></div><div class="sim-versus" aria-hidden="true"><span>VS</span><i></i></div><div class="sim-deck-pick"><span class="sim-pick-label">Adversaires</span><label><select id="sim-opponent" multiple size="5" required><optgroup label="Références d’entraînement · format libre">${simulationDeckOptions(references)}</optgroup><optgroup label="Mes decks">${simulationDeckOptions(state.decks)}</optgroup></select></label><small>Maintenez Ctrl/Cmd pour sélectionner plusieurs decks</small></div></div>
      <p class="sim-note">Duels BO1 sans commandant : seul le deck principal est joué, sans réserve. Les références sont des listes d’entraînement, sans garantie de légalité Arena. Vous pouvez aussi choisir un de vos decks comme adversaire.</p>
      <div class="sim-options"><label>Nombre de parties<select id="sim-games"><option value="10">10 · échauffement</option><option value="100" selected>100 · duel approfondi</option><option value="500">500 · série longue</option><option value="1000">1 000 · laboratoire</option></select></label>
      <details><summary>Paramètres de l’expérience</summary><div class="sim-advanced"><label>Graine aléatoire<input id="sim-seed" type="number" min="0" max="4294967295" step="1" required value="${Math.floor(Math.random()*1000000)}"></label><label>Secondes maximum par partie<input id="sim-timeout" type="number" min="10" max="300" step="1" required value="120"></label></div><p>Les sièges alternent ; Forge décide du premier joueur. La graine est conservée, sans garantie de reproduction identique entre versions du moteur.</p></details></div>
      <div class="sim-launch"><button id="sim-start" class="action-primary sim-launch-button" type="submit" ${!engine.ready||engine.activeId||!state.decks.length?'disabled':''}><span>Lancer la simulation</span><span aria-hidden="true">→</span></button><p>Une série à la fois · jusqu’à 2 Go de mémoire Java · aucun appel Gemini</p></div><p id="sim-form-feedback" role="status">${!state.decks.length?'Synchronisez vos decks pour préparer un duel.':engine.activeId?'Une série est déjà en cours. Consultez-la ou annulez-la ci-dessous.':''}</p></form>
      <section id="sim-lab" class="sim-lab plate"><div class="sim-section-heading"><div><span class="sim-kicker">Deck Lab · Standard</span><h2>Trouver une meilleure version</h2><p>Gemini propose des variantes réalisables avec votre collection, puis Forge les confronte aux adversaires choisis.</p></div><span class="sim-bo1">Collection verrouillée</span></div>
      <form id="deck-lab-form" onsubmit="startDeckLab(event)"><div class="sim-lab-grid"><label>Deck de départ<select id="lab-deck" required>${standardDeckOptions(state.decks,simulation.preferredDeck||state.currentDeckId)}</select></label><label>Decks de référence<select id="lab-opponents" multiple size="5" required><optgroup label="Références d’entraînement">${simulationDeckOptions(references)}</optgroup><optgroup label="Mes decks Standard">${standardDeckOptions(state.decks)}</optgroup></select></label><label>Candidats par itération<select id="lab-candidates"><option>1</option><option selected>2</option><option>3</option></select></label><label>Parties par matchup<input id="lab-games" type="number" min="5" max="1000" value="50" required></label><label>Seuil de validation (%)<input id="lab-threshold" type="number" min="1" max="100" value="60" required></label><label>Workers Forge<select id="lab-workers"><option>1</option><option selected>2</option><option>3</option><option>4</option></select></label><label>Budget Gemini / itération<select id="lab-tokens"><option value="2048" selected>2 048 tokens</option><option value="3072">3 072 tokens</option><option value="4096">4 096 tokens</option></select></label><label>Itérations maximum<input id="lab-iterations" type="number" min="1" value="3" required></label></div><label class="sim-toggle"><input id="lab-auto" type="checkbox" checked><span>Itérer jusqu’au seuil demandé</span></label><div class="sim-launch"><button id="lab-start" class="action-primary sim-launch-button" type="submit" ${!engine.ready||!standardDeckOptions(state.decks)?'disabled':''}><span>Démarrer le Deck Lab</span><span aria-hidden="true">→</span></button><p>Budget maximal prévisible : <strong id="lab-budget-preview">6 144 tokens</strong></p></div><p id="lab-feedback" role="status"></p></form><div id="lab-results"></div></section>
      <section id="sim-random-lab" class="sim-lab plate"><div class="sim-section-heading"><div><span class="sim-kicker">Générateur local · sans Gemini</span><h2>Explorer des constructions</h2><p>Créez des variantes depuis votre collection, puis jouez une partie contre chaque référence sélectionnée.</p></div><span class="sim-bo1">Forge uniquement</span></div>
      <form id="random-lab-form" onsubmit="startRandomLab(event)"><div class="sim-lab-grid random-lab-grid"><label>Construction<select id="random-mode" onchange="updateRandomLabMode()"><option value="mono">Une couleur · 24 terrains</option><option value="bicolor">Bicolore · 22 terrains</option></select></label><label>Couleur principale<select id="random-color-a"><option value="W">Blanc</option><option value="U">Bleu</option><option value="B">Noir</option><option value="R">Rouge</option><option value="G">Vert</option></select></label><label id="random-color-b-wrap">Deuxième couleur<select id="random-color-b"><option value="U">Bleu</option><option value="W">Blanc</option><option value="B">Noir</option><option value="R">Rouge</option><option value="G">Vert</option></select></label><label>Terrains<input id="random-lands" type="number" min="0" max="60" value="24" required></label><label>Terrains doubles<input id="random-duals" type="number" min="0" max="30" value="8" required></label><label>Terrains de chaque couleur<input id="random-per-color" type="number" min="0" max="30" value="7" required></label><label>Créatures<input id="random-creatures" type="number" min="0" max="60" value="20" required></label><label>Non-créatures<input id="random-spells" type="number" min="0" max="60" value="16" required></label><label>Combinaisons à tester<input id="random-candidates" type="number" min="1" value="20" required></label><label>Decks de référence<select id="random-opponents" multiple size="5" required><optgroup label="Références d’entraînement">${simulationDeckOptions(references)}</optgroup><optgroup label="Mes decks">${simulationDeckOptions(state.decks)}</optgroup></select></label></div><p class="sim-constraint-total">Total construit : <strong id="random-total">60 / 60</strong> cartes · une partie Forge par référence et par variante</p><div class="sim-launch"><button id="random-start" class="action-primary sim-launch-button" type="submit" ${!engine.ready?'disabled':''}><span>Générer et challenger</span><span aria-hidden="true">→</span></button><p id="random-lab-feedback" role="status"></p></div></form><div id="random-lab-results"></div></section>
      <section id="sim-result" aria-label="Résultats de simulation"></section><section id="random-history" class="random-history"><div class="random-history-heading"><div><span class="sim-kicker">Générateur local</span><h2>Meilleures générations</h2><p>Les variantes sont classées par performance Forge, avec le contexte exact de leur construction.</p></div><button type="button" class="text-button" onclick="showSimulationSection('random')">Nouvelle génération →</button></div><div id="random-history-list"></div></section><section id="sim-history" class="sim-history"><h2>Votre carnet d’expériences</h2><p>Chaque série conserve sa date et les deux listes jouées. Ces résultats ne modifient pas vos statistiques Arena.</p><div id="sim-history-list"></div></section>`;
    organizeSimulationScreens(container);
    const opponentPicker=document.getElementById('sim-opponent');
    if(opponentPicker && !opponentPicker.selectedOptions.length && opponentPicker.options.length) opponentPicker.options[0].selected=true;
    const labOpponents=document.getElementById('lab-opponents'); if(labOpponents) [...labOpponents.options].slice(0,3).forEach(option=>option.selected=true);
    const randomOpponents=document.getElementById('random-opponents'); if(randomOpponents) [...randomOpponents.options].slice(0,3).forEach(option=>option.selected=true);
    const labTokens=document.getElementById('lab-tokens'); if(labTokens) { labTokens.innerHTML='<option value="2048">2 048 tokens</option><option value="4096">4 096 tokens</option><option value="8192" selected>8 192 tokens</option><option value="16384">16 384 tokens</option><option value="32768">32 768 tokens</option><option value="65536">65 536 tokens</option>'; }
    ['lab-tokens','lab-iterations','lab-auto'].forEach(id=>document.getElementById(id)?.addEventListener('change',updateLabBudget)); updateLabBudget();
    ['random-lands','random-duals','random-per-color','random-creatures','random-spells'].forEach(id=>document.getElementById(id)?.addEventListener('input',updateRandomLabTotal));
    updateRandomLabMode();
    renderSimulationHistory();
    renderRandomLabHistory();
    const id=engine.activeId||simulation.selected?.id||reports[0]?.id;
    if(id) await loadSimulation(id);
    syncSimulationNavigation();
    if(simulation.pendingSection) { const section=simulation.pendingSection; simulation.pendingSection=null; focusSimulationSection(section); }
  } catch(error) {
    if(generation!==simulation.generation) return;
    container.innerHTML=heading('','Simulation')+`<p role="alert">${escapeHtml(error.message)}</p><button class="text-button" onclick="renderSimulation()">Réessayer</button>`;
  }
}
async function saveSimulationEngine(event) {
  event.preventDefault();const button=event.submitter;button.disabled=true;
  const feedback=document.getElementById('sim-engine-feedback');
  try {
    simulation.engine=await api('/api/simulation-engine',{method:'PUT',headers:{'content-type':'application/json'},body:JSON.stringify({forgeDir:document.getElementById('forge-dir').value,javaPath:document.getElementById('forge-java').value})});
    feedback.textContent=simulation.engine.problem||'Forge disponible. Vous pouvez lancer votre duel.';
    document.getElementById('sim-engine-state').textContent=simulation.engine.ready?'Forge disponible':'Forge à configurer';
    document.getElementById('sim-start').disabled=!simulation.engine.ready||!!simulation.engine.activeId||!state.decks.length;
  } catch(error) {feedback.textContent=error.message;} finally {button.disabled=false;}
}
async function startSimulation(event) {
  event.preventDefault();const button=event.submitter;button.disabled=true;
  const feedback=document.getElementById('sim-form-feedback');
  try {
    const request={deckId:document.getElementById('sim-deck').value,opponentIds:[...document.getElementById('sim-opponent').selectedOptions].map(option=>option.value),games:Number(document.getElementById('sim-games').value),seed:Number(document.getElementById('sim-seed').value),secondsPerGame:Number(document.getElementById('sim-timeout').value)};
    const report=await api('/api/simulations',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(request)});
    simulation.engine.activeId=report.id;
    feedback.textContent='Série lancée. Vous pouvez continuer à explorer votre collection.';
    focusSimulationSection('reports',false);
    await loadSimulation(report.id);
    document.getElementById('sim-result')?.scrollIntoView({block:'start',behavior:reduceMotion?'instant':'smooth'});
  } catch(error) {feedback.textContent=error.message;button.disabled=false;}
}
function updateLabBudget() {
  const tokens=Number(document.getElementById('lab-tokens')?.value||0), iterations=document.getElementById('lab-auto')?.checked?Number(document.getElementById('lab-iterations')?.value||1):1;
  const node=document.getElementById('lab-budget-preview'); if(node) node.textContent=`${(tokens*iterations).toLocaleString('fr-FR')} tokens`;
}
function updateRandomLabMode() {
  const bicolor=document.getElementById('random-mode')?.value==='bicolor';
  const mode=document.getElementById('random-mode');
  const duals=document.getElementById('random-duals'), perColor=document.getElementById('random-per-color'), lands=document.getElementById('random-lands'), wrap=document.getElementById('random-color-b-wrap');
  if(!mode||!duals||!perColor||!lands) return;
  wrap?.classList.toggle('is-visible',bicolor);
  duals.disabled=!bicolor; perColor.disabled=!bicolor; lands.disabled=bicolor;
  if(bicolor) { duals.value=8; perColor.value=7; lands.value=22; }
  else { lands.value=24; duals.value=8; perColor.value=7; }
  updateRandomLabTotal();
}
function updateRandomLabTotal() {
  const bicolor=document.getElementById('random-mode')?.value==='bicolor';
  const lands=bicolor?Number(document.getElementById('random-duals')?.value||0)+2*Number(document.getElementById('random-per-color')?.value||0):Number(document.getElementById('random-lands')?.value||0);
  const total=lands+Number(document.getElementById('random-creatures')?.value||0)+Number(document.getElementById('random-spells')?.value||0);
  const node=document.getElementById('random-total'); if(node) { node.textContent=`${total} / 60`; node.classList.toggle('is-invalid',total!==60); }
}
function randomLabMatchups(item) {
  const report=item.report;
  if(!report) return '<p class="sim-note">Forge attend une place disponible.</p>';
  const opponents=report.opponents?.length?report.opponents:[report.opponent];
  return opponents.map((opponent,index)=>{const game=report.results.find(result=>result.opponentIndex===index), outcome=game?simOutcome[game.outcome]:'En attente'; return `<span class="random-matchup"><strong>${escapeHtml(opponent.name)}</strong><span class="${game?.outcome||'pending'}">${outcome}</span></span>`;}).join('');
}
function renderRandomLabResults(data) {
  const node=document.getElementById('random-lab-results'); if(!node) return;
  const running=data.status!=='completed';
  node.innerHTML=`<div class="random-results-heading"><div><span class="sim-kicker">Classement local</span><h3>${running?'Forge est en train de comparer…':'Résultats des variantes'}</h3></div><span class="sim-note">${data.candidates.filter(item=>item.status==='completed').length} / ${data.candidates.length} variantes terminées</span></div><div class="random-ranking">${data.candidates.map((item,index)=>{const score=item.score===null?null:Math.round(item.score*100), decisive=item.wins+item.losses, winPct=decisive?Math.round(item.wins/decisive*100):0; return `<article class="random-candidate ${index===0&&score!==null?'is-best':''}"><div class="random-candidate-head"><div><span class="random-rank">#${index+1}</span><strong>${escapeHtml(item.deck.name)}</strong><p>${item.wins} victoire${item.wins>1?'s':''} · ${item.losses} défaite${item.losses>1?'s':''} · ${item.draws} égalité${item.draws>1?'s':''}</p></div><strong class="random-score">${score===null?'En attente':`${score} %`}</strong></div><div class="sim-matchup-bar" role="img" aria-label="${winPct} % de victoires"><span class="sim-matchup-wins" style="width:${winPct}%"></span><span class="sim-matchup-losses" style="width:${100-winPct}%"></span></div><div class="random-matchups">${randomLabMatchups(item)}</div><details class="random-deck-details"><summary>Voir les ${item.deck.cardCount} cartes</summary><div class="random-card-list">${item.deck.mainDeck.map(card=>`<span>${card.quantity} · ${escapeHtml(card.name)}</span>`).join('')}</div></details>${item.reportId?`<button type="button" class="text-button" data-id="${escapeHtml(item.reportId)}" onclick="openSimulationReport(this.dataset.id)">Voir le match détaillé</button>`:''}${item.error?`<p class="sim-error">${escapeHtml(item.error)}</p>`:''}</article>`;}).join('')}</div>`;
}
function renderRandomLabHistory() {
  const node=document.getElementById('random-history-list'); if(!node) return;
  const labels={W:'Blanc',U:'Bleu',B:'Noir',R:'Rouge',G:'Vert'}, entries=[];
  (simulation.randomReports||[]).forEach(campaign=>(campaign.candidates||[]).forEach(item=>{
    if(typeof item.score==='number') entries.push({campaign,item});
  }));
  entries.sort((a,b)=>b.item.score-a.item.score);
  node.innerHTML=entries.slice(0,20).map(({campaign,item},index)=>{
    const request=campaign.request||{}, colors=(request.colors||[]).map(color=>labels[color]||color).join(' + ');
    const constraints=request.bicolor
      ? `${request.dualLandCount} doubles · ${request.landsPerColor}/${request.landsPerColor} terrains · ${request.creatureCount} créatures · ${request.noncreatureCount} non-créatures`
      : `${request.landCount} terrains · ${request.creatureCount} créatures · ${request.noncreatureCount} non-créatures`;
    const refs=(campaign.opponents||[]).map(opponent=>opponent.name).join(' · ')||'Aucune référence';
    const score=Math.round(item.score*100), games=item.wins+item.losses+item.draws;
    return `<article class="random-history-row"><span class="random-history-rank">#${index+1}</span><div><strong>${escapeHtml(item.deck?.name||'Variante sans nom')}</strong><p>${score} % · ${item.wins} V / ${item.losses} D / ${item.draws} N · ${games} partie${games>1?'s':''}</p><p class="random-history-context">${escapeHtml(new Date((campaign.createdAt||0)*1000).toLocaleString('fr-FR'))} · Collection locale · ${escapeHtml(request.bicolor?'Bicolore':'Mono-couleur')} ${escapeHtml(colors)} · ${escapeHtml(constraints)}<br>Références : ${escapeHtml(refs)}</p></div><div class="random-history-score">${score} %${item.reportId?`<button type="button" class="text-button" data-id="${escapeHtml(item.reportId)}" onclick="openSimulationReport(this.dataset.id)">Détail Forge</button>`:''}</div></article>`;
  }).join('')||'<p class="sim-note">Les générations terminées apparaîtront ici après leurs premières parties Forge.</p>';
}
async function loadRandomLab(id) {
  clearTimeout(simulation.randomLabTimer);
  try {
    const data=await api(`/api/random-lab/${encodeURIComponent(id)}`);
    if(state.currentView!=='simulation') return;
    simulation.randomLabId=id; renderRandomLabResults(data);
    const existing=(simulation.randomReports||[]).findIndex(item=>item.id===data.id);
    if(existing>=0) simulation.randomReports[existing]=data; else simulation.randomReports.unshift(data);
    renderRandomLabHistory();
    if(data.status!=='completed') simulation.randomLabTimer=setTimeout(()=>loadRandomLab(id),1200);
  } catch(error) { const feedback=document.getElementById('random-lab-feedback'); if(feedback) feedback.textContent=error.message; }
}
async function startRandomLab(event) {
  event.preventDefault();
  const button=event.submitter, feedback=document.getElementById('random-lab-feedback'); button.disabled=true;
  const bicolor=document.getElementById('random-mode').value==='bicolor';
  const opponentIds=[...document.getElementById('random-opponents').selectedOptions].map(option=>option.value);
  const request={colors:[document.getElementById('random-color-a').value,...(bicolor?[document.getElementById('random-color-b').value]:[])],bicolor,landCount:Number(document.getElementById('random-lands').value),dualLandCount:Number(document.getElementById('random-duals').value),landsPerColor:Number(document.getElementById('random-per-color').value),creatureCount:Number(document.getElementById('random-creatures').value),noncreatureCount:Number(document.getElementById('random-spells').value),candidateCount:Number(document.getElementById('random-candidates').value),opponentIds,seed:Math.floor(Math.random()*Number.MAX_SAFE_INTEGER)};
  try {
    updateRandomLabTotal();
    if(!opponentIds.length) throw new Error('Sélectionnez au moins un deck de référence.');
    const campaign=await api('/api/random-lab',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(request)});
    simulation.randomLabId=campaign.id; focusSimulationSection('random',false); feedback.textContent='Variantes générées. Forge lance les matchups en file d’attente.'; await loadRandomLab(campaign.id);
    document.getElementById('random-lab-results')?.scrollIntoView({block:'start',behavior:reduceMotion?'auto':'smooth'});
  } catch(error) { feedback.textContent=error.message; }
  finally { button.disabled=false; }
}
async function waitForLabReports(ids, onProgress) {
  while(true) {
    const reports=await Promise.all(ids.map(id=>api(`/api/simulations/${encodeURIComponent(id)}`)));
    onProgress(reports);
    if(reports.every(report=>!simActive(report))) return reports;
    await new Promise(resolve=>setTimeout(resolve,1500));
  }
}
function deckLabRate(report) {
  const wins=report.results.filter(game=>game.outcome==='win').length, losses=report.results.filter(game=>game.outcome==='loss').length;
  return wins+losses?wins/(wins+losses):0;
}
function renderDeckLabRound(rounds, threshold) {
  const node=document.getElementById('lab-results'); if(!node) return;
  node.innerHTML=rounds.map((round,index)=>`<section class="lab-round"><h3>Itération ${index+1}</h3>${round.map(item=>{const rate=item.report?deckLabRate(item.report):0,pct=Math.round(rate*100),passed=pct>=threshold;return `<article class="lab-candidate ${passed?'is-passed':''}"><div><strong>${escapeHtml(item.candidate.deck.name)}</strong><p>${escapeHtml(item.candidate.rationale)}</p></div><div class="lab-candidate-score"><strong>${item.report?`${pct} %`:'En attente'}</strong><span>${passed?'Seuil atteint':'Objectif '+threshold+' %'}</span></div>${item.report?`<div class="sim-matchup-bar" aria-label="${pct} % de victoires"><span class="sim-matchup-wins" style="width:${pct}%"></span><span class="sim-matchup-losses" style="width:${100-pct}%"></span></div><button class="text-button" data-id="${escapeHtml(item.report.id)}" onclick="openSimulationReport(this.dataset.id)">Voir le rapport détaillé</button>`:''}</article>`;}).join('')}</section>`).join('');
}
async function startDeckLab(event) {
  event.preventDefault(); if(simulation.labRunning) return; simulation.labRunning=true;
  const button=event.submitter, feedback=document.getElementById('lab-feedback'); button.disabled=true;
  const deckId=document.getElementById('lab-deck').value, opponentIds=[...document.getElementById('lab-opponents').selectedOptions].map(option=>option.value), candidateCount=Number(document.getElementById('lab-candidates').value), games=Number(document.getElementById('lab-games').value), threshold=Number(document.getElementById('lab-threshold').value), workers=Number(document.getElementById('lab-workers').value), maxOutputTokens=Number(document.getElementById('lab-tokens').value), auto=document.getElementById('lab-auto').checked, maxIterations=auto?Number(document.getElementById('lab-iterations').value):1, rounds=[]; let baseDeck=null,simulationFeedback=null,cacheName=null,cacheAttempted=false;
  try {
    if(!opponentIds.length) throw new Error('Sélectionnez au moins un deck de référence.');
    for(let iteration=0;iteration<maxIterations;iteration++) {
      feedback.textContent=`Itération ${iteration+1}/${maxIterations} · Gemini construit ${candidateCount} candidat(s)…`;
      const generation=await api('/api/deck-lab/candidates',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({deckId,baseDeck,candidateCount,maxOutputTokens,simulationFeedback,cacheName,cacheAttempted})}); cacheName=generation.cacheName; cacheAttempted=generation.cacheAttempted;
      const round=generation.candidates.map(candidate=>({candidate,report:null})); rounds.push(round); renderDeckLabRound(rounds,threshold);
      for(let offset=0;offset<round.length;offset+=workers) {
        const batch=round.slice(offset,offset+workers);
        feedback.textContent=`Itération ${iteration+1}/${maxIterations} · simulations Forge ${offset+1}–${offset+batch.length}/${round.length}…`;
        const started=await Promise.all(batch.map(item=>api('/api/simulations',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({deckId:item.candidate.deck.id,deck:item.candidate.deck,opponentIds,games,seed:Math.floor(Math.random()*4294967295),secondsPerGame:120,parallel:true})})));
        const reports=await waitForLabReports(started.map(report=>report.id), partial=>{partial.forEach((report,index)=>batch[index].report=report);renderDeckLabRound(rounds,threshold);});
        reports.forEach((report,index)=>batch[index].report=report);
      }
      renderDeckLabRound(rounds,threshold);
      const bestItem=round.reduce((best,item)=>!best||deckLabRate(item.report)>deckLabRate(best.report)?item:best,null), best=deckLabRate(bestItem.report); baseDeck=bestItem.candidate.deck;
      simulationFeedback=`Le meilleur candidat a obtenu ${Math.round(best*100)} % sur ${bestItem.report.results.length} parties. Matchups : ${bestItem.report.opponents.map((deck,index)=>{const games=bestItem.report.results.filter(game=>game.opponentIndex===index),wins=games.filter(game=>game.outcome==='win').length,losses=games.filter(game=>game.outcome==='loss').length;return `${deck.name} (${wins} V, ${losses} D)`;}).join(', ')}. Améliore surtout les matchups faibles avec des cartes de la collection.`;
      if(best*100>=threshold) { feedback.textContent=`Critère atteint : meilleur candidat à ${Math.round(best*100)} %.`; break; }
      if(!auto) { feedback.textContent='Simulation unique terminée. Le seuil n’a pas été atteint.'; break; }
      if(iteration===maxIterations-1) feedback.textContent=`Limite atteinte : meilleur score ${Math.round(best*100)} %, sous le seuil demandé.`;
    }
    simulation.reports=await api('/api/simulations'); renderSimulationHistory();
  } catch(error) { feedback.textContent=error.message; }
  finally { simulation.labRunning=false; button.disabled=false; }
}
async function loadSimulation(id) {
  clearTimeout(simulation.timer);const generation=++simulation.generation;
  try {
    const report=await api(`/api/simulations/${encodeURIComponent(id)}`);
    if(generation!==simulation.generation||state.currentView!=='simulation') return;
    if(simulation.selected?.id!==report.id) simulation.selectedGame=null;
    simulation.selected=report;renderSimulationReport(report);
    if(simActive(report)) simulation.timer=setTimeout(()=>loadSimulation(id),1500);
    else {
      // Another campaign may still be running while this historical report is inspected.
      const engine=await api('/api/simulation-engine');
      if(generation!==simulation.generation) return;
      simulation.engine=engine;
      document.getElementById('sim-start').disabled=!engine.ready||!!engine.activeId||!state.decks.length;
      simulation.reports=await api('/api/simulations');
      if(generation===simulation.generation) renderSimulationHistory();
    }
  } catch(error) {
    if(generation===simulation.generation) document.getElementById('sim-result').innerHTML=`<p role="alert">${escapeHtml(error.message)}</p><button class="text-button" data-id="${escapeHtml(id)}" onclick="loadSimulation(this.dataset.id)">Réessayer</button>`;
  }
}
function renderSimulationReport(report) {
  const panel=document.getElementById('sim-result');
  const signature=JSON.stringify([report.id,report.status,report.engineVersion,report.results.length,report.error]);
  if(panel.dataset.signature===signature) return;
  const protocolOpen=panel.querySelector('.sim-protocol')?.open;
  const focused=panel.contains(document.activeElement)?document.activeElement.getAttribute('onclick'):null;
  panel.dataset.signature=signature;
  const wins=report.results.filter(g=>g.outcome==='win').length,losses=report.results.filter(g=>g.outcome==='loss').length;
  const draws=report.results.filter(g=>g.outcome==='draw').length,timeouts=report.results.filter(g=>g.outcome==='timeout').length;
  const n=wins+losses,p=n?wins/n:0,z2=1.96**2,center=n?(p+z2/(2*n))/(1+z2/n):0,radius=n?1.96*Math.sqrt((p*(1-p)+z2/(4*n))/n)/(1+z2/n):0;
  const finished=report.results.filter(g=>g.outcome!=='timeout'),average=finished.length?(finished.reduce((sum,g)=>sum+g.turns,0)/finished.length).toFixed(1):'—';
  const opponents=report.opponents?.length?report.opponents:[report.opponent];
  const matchupHtml=opponents.map((opponent,index)=>{const games=report.results.filter(g=>g.opponentIndex===index||(!g.opponentIndex&&index===0));const w=games.filter(g=>g.outcome==='win').length,l=games.filter(g=>g.outcome==='loss').length,d=games.length-w-l,decisive=w+l,pct=decisive?Math.round(w/decisive*100):0;return `<article class="sim-matchup"><div class="sim-matchup-head"><div><h3>${escapeHtml(opponent.name)}</h3><p>${escapeHtml(opponent.format||'Format non précisé')} · ${opponent.cardCount||opponent.mainDeck?.reduce((sum,c)=>sum+c.quantity,0)||0} cartes</p></div><strong>${decisive?`${pct} %`:'—'}</strong></div><div class="sim-matchup-bar" role="img" aria-label="${pct} % de victoires contre ${escapeHtml(opponent.name)}"><span class="sim-matchup-wins" style="width:${pct}%"></span><span class="sim-matchup-losses" style="width:${100-pct}%"></span></div><div class="sim-matchup-legend"><span><i class="win"></i>${w} victoires</span><span><i class="loss"></i>${l} défaites</span><span>${d} autres</span></div></article>`;}).join('');
  panel.innerHTML=`<article class="sim-report plate"><div class="sim-report-heading"><div><span class="sim-kicker">Carnet d’expérience</span><h2>${escapeHtml(report.deck.name)} <span>contre ${opponents.length} adversaire${opponents.length>1?'s':''}</span></h2><p>${escapeHtml(new Date(report.createdAt*1000).toLocaleString('fr-FR'))} · ${escapeHtml(report.engineVersion||'Initialisation de Forge')}</p></div><span class="sim-status ${report.status}" role="status"><i></i>${simStatuses[report.status]||escapeHtml(report.status)}</span></div>
    <div class="sim-progress-label"><span>${report.results.length} / ${report.gamesRequested} parties traitées</span>${simActive(report)?'<button class="text-button" onclick="cancelSimulation()">Annuler la série</button>':''}</div><progress max="${report.gamesRequested}" value="${report.results.length}" aria-label="Progression des simulations"></progress>
    ${report.error?`<p class="sim-error" role="alert">${escapeHtml(report.error)}</p>`:''}
    <div class="sim-score"><div class="sim-score-main"><p>Avantage actuel</p><strong>${n?simPct(p):'—'}</strong><p>${n?`${wins} victoires · ${losses} défaites`:'Les résultats apparaîtront après les premières parties.'}</p></div><div class="sim-tally" aria-label="Répartition des résultats"><span style="--value:${report.gamesRequested?wins/report.gamesRequested:0}" class="win">${wins}<small>Victoires</small></span><span style="--value:${report.gamesRequested?losses/report.gamesRequested:0}" class="loss">${losses}<small>Défaites</small></span><span class="draw">${draws}<small>Égalités</small></span><span class="timeout">${timeouts}<small>Délais</small></span></div><dl><div><dt>Tours moyens</dt><dd>${average}</dd></div></dl></div>
    <p class="sim-note">${n?`Intervalle indicatif à 95 % : ${simPct(Math.max(0,center-radius))} à ${simPct(Math.min(1,center+radius))}. `:''}Ce taux décrit ce duel entre IA. Égalités et délais sont exclus du taux ; une erreur ne compte jamais comme une défaite. L’IA peut mal piloter les combos complexes.</p>
    <section class="sim-matchups"><div class="sim-section-heading"><div><span class="sim-kicker">Lecture comparative</span><h3>Performance par adversaire</h3></div><span class="sim-bar-key"><i class="win"></i>Victoire <i class="loss"></i>Défaite</span></div>${matchupHtml}</section>
    <h3>Le fil des parties</h3><p class="sim-note">V : victoire · D : défaite · É : égalité · T : délai dépassé. Sélectionnez une partie pour ses détails.</p><div class="sim-game-grid">${report.results.map(g=>`<button class="sim-game ${g.outcome}" onclick="selectSimulationGame(${g.index})" aria-label="Partie ${g.index} : ${simOutcome[g.outcome]}">${g.index}<span>${{win:'V',loss:'D',draw:'É',timeout:'T'}[g.outcome]}</span></button>`).join('')||'<p>Aucune partie terminée pour le moment.</p>'}</div><p id="sim-game-detail" class="sim-game-detail" role="status"></p>
    <details class="sim-protocol"><summary>Protocole et listes exactes</summary><p>BO1, sans réserve ni commandant. Sièges alternés, choix du premier joueur par Forge. Graine ${report.seed} ; délai ${report.secondsPerGame} s par partie. Les cartes introuvables arrêtent la série ; aucune version rééquilibrée n’est remplacée par sa version papier. Le moteur ne certifie pas la légalité du format.</p><div class="sim-snapshot"><section><h3>${escapeHtml(report.deck.name)}</h3><pre>${escapeHtml(simulationDeckText(report.deck))}</pre></section><section><h3>${escapeHtml(report.opponent.name)}</h3><pre>${escapeHtml(simulationDeckText(report.opponent))}</pre></section></div></details>
    <footer class="sim-report-actions"><button class="text-button" onclick="downloadSimulation()">Télécharger le rapport JSON</button><a class="text-button" href="/api/simulations/${encodeURIComponent(report.id)}/logs">Journal du moteur</a></footer></article>`;
  panel.querySelector('.sim-protocol').open=!!protocolOpen;
  if(simulation.selectedGame) selectSimulationGame(simulation.selectedGame);
  if(focused) [...panel.querySelectorAll('button[onclick]')].find(button=>button.getAttribute('onclick')===focused)?.focus({preventScroll:true});
}
function selectSimulationGame(index) {
  const game=simulation.selected.results.find(g=>g.index===index);
  if(!game) {simulation.selectedGame=null;return;}
  simulation.selectedGame=index;
  document.getElementById('sim-game-detail').textContent=`Partie ${index} : ${simOutcome[game.outcome]} · ${game.turns} tours · ${(game.durationMs/1000).toFixed(1)} s de calcul · siège ${game.seat}.`;
}
function simulationDeckText(deck) {return deck.mainDeck.map(card=>`${card.quantity} ${card.name}`).join('\n');}
function renderSimulationHistory() {
  const node=document.getElementById('sim-history-list');if(!node) return;
  node.innerHTML=simulation.reports.map(report=>`<button class="sim-history-row" data-id="${escapeHtml(report.id)}" onclick="openSimulationReport(this.dataset.id)"><span><strong>${escapeHtml(report.deckName)}</strong><span>contre ${escapeHtml(report.opponentNames?.join(' · ')||report.opponentName)}</span></span><span>${escapeHtml(new Date(report.createdAt*1000).toLocaleString('fr-FR'))}<span>${simStatuses[report.status]} · ${report.completed}/${report.gamesRequested} parties</span></span><strong>${report.winRate===null?'—':simPct(report.winRate)}</strong></button>`).join('')||'<p class="sim-note">Votre première série ouvrira ce carnet. Choisissez un adversaire et commencez par 10 parties.</p>';
}
async function cancelSimulation() {
  try {await api(`/api/simulations/${encodeURIComponent(simulation.selected.id)}/cancel`,{method:'POST'});await loadSimulation(simulation.selected.id);}
  catch(error) {showAlert(error.message,true);}
}
function downloadSimulation() {
  const report=simulation.selected,url=URL.createObjectURL(new Blob([JSON.stringify(report,null,2)],{type:'application/json'}));
  const link=document.createElement('a');link.href=url;link.download=`simulation-${report.id}.json`;link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);
}
