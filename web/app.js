const state = { status: null, decks: [], collection: [], currentView: 'home', currentDeckId: null, onlyUserDecks: false, scryfallCache: new Map() };
const escapeHtml = value => String(value ?? '').replace(/[&<>'"]/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;',"'":'&#39;','"':'&quot;'}[char]));
const api = async (url, options = {}) => {
  const response = await fetch(url, options);
  if (!response.ok) {
    const payload = await response.json().catch(() => ({}));
    throw new Error(payload.error || `Request failed (${response.status})`);
  }
  return response.json();
};
const motionApi = window.Motion || {};
const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
function animateElement(element, keyframes, options = {}) {
  if (!element || reduceMotion || typeof motionApi.animate !== 'function') return;
  motionApi.animate(element, keyframes, { duration: 0.28, ease: 'easeOut', ...options });
}
function animateView(view) {
  if (!view || reduceMotion || typeof motionApi.animate !== 'function') return;
  animateElement(view, { opacity: [0.35, 1], y: [8, 0] }, { duration: 0.24 });
  const tiles = view.querySelectorAll('.deck-tile');
  if (tiles.length && typeof motionApi.stagger === 'function') {
    motionApi.animate(tiles, { opacity: [0, 1], y: [10, 0] }, { delay: index => Math.min(index * 0.025, 0.15), duration: 0.3, ease: 'easeOut' });
  }
}
function tapFeedback(element) { animateElement(element, { scale: [1, 0.97, 1] }, { duration: 0.18 }); }
const metric = (label, value, hint = '') => `<article class="plate plate-enter p-5"><p class="eyebrow">${escapeHtml(label)}</p><p class="mt-2 text-3xl font-bold text-chalk">${escapeHtml(value)}</p><p class="mt-1 text-xs text-slate-500">${escapeHtml(hint)}</p></article>`;
const heading = (_eyebrow, title, copy = '') => `<header class="mb-7 plate-enter"><h1 class="text-4xl font-bold text-chalk sm:text-5xl">${escapeHtml(title)}</h1>${copy ? `<p class="mt-2 max-w-2xl text-sm leading-6 text-slate-400">${escapeHtml(copy)}</p>` : ''}</header>`;
const formatTime = value => value ? new Date(Number(value) * 1000).toLocaleString() : 'Jamais';
const formatDeckDate = value => value && !String(value).startsWith('0001-01-01') ? new Date(String(value).replace(/^"|"$/g, '')).toLocaleString() : 'Non renseigné';

function showAlert(message, error = false) {
  const alert = document.getElementById('alert');
  alert.textContent = message;
  alert.className = `mb-6 rounded-xl border px-4 py-3 text-sm ${error ? 'border-red-500/30 bg-red-500/10 text-red-200' : 'border-emerald-500/30 bg-emerald-500/10 text-emerald-200'}`;
  animateElement(alert, { opacity: [0, 1], y: [-6, 0] }, { duration: 0.2 });
  clearTimeout(showAlert.timer);
  showAlert.timer = setTimeout(() => alert.classList.add('hidden'), 5000);
}

function showView(view) {
  state.currentView = view;
  document.querySelectorAll('[data-view]').forEach(element => element.classList.add('hidden'));
  document.getElementById(`view-${view}`).classList.remove('hidden');
  document.querySelectorAll('[data-nav]').forEach(button => button.classList.toggle('nav-active', button.dataset.nav === view));
  if (view === 'collection') renderCollection();
  animateView(document.getElementById(`view-${view}`));
}

function renderHome() {
  const status = state.status;
  document.getElementById('view-home').innerHTML = heading('Dashboard', 'Votre bibliothèque Arena', 'Collectionnez vos coups de cœur. Explorez vos synergies. Préparez votre prochaine partie.') + `
    <div class="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
      ${metric('Decks', status.deckCount, 'decks disponibles')}
      ${metric('Cartes différentes', status.cardsOwned, `${status.totalCopies} total copies`)}
      ${metric('Jokers rares', status.wildcards.rare, `${status.wildcards.mythic} mythic`) }
      ${metric('Dernière synchro', formatTime(status.syncedAt), status.logExists ? 'log file connected' : 'log file not found')}
    </div>
    ${status.warnings.length ? `<div class="plate-soft mt-6 border-amber-400/30 p-5"><h2 class="font-semibold text-amber-200">Parser notes</h2><ul class="mt-2 space-y-1 text-sm text-amber-100/70">${status.warnings.map(item => `<li><span class="mr-2 text-amber-300">/</span>${escapeHtml(item)}</li>`).join('')}</ul></div>` : ''}
    <div class="mt-8 flex items-center justify-between"><h2 class="text-xl font-bold text-chalk">Vos decks</h2><button onclick="showView('decks')" class="text-sm font-semibold text-gold">Tous les decks</button></div>
    <div class="mt-4 grid gap-4 md:grid-cols-2 xl:grid-cols-3">${state.decks.slice(0, 6).map(deckCard).join('') || empty('Ouvrez vos decks dans MTGA puis synchronisez.')}</div>`;
}

function colorPips(colors) {
  const styles = {W:'bg-amber-50 text-slate-900', U:'bg-blue-500 text-white', B:'bg-slate-900 text-white', R:'bg-red-500 text-white', G:'bg-emerald-600 text-white'};
  return colors.length ? colors.map(color => `<span class="inline-flex h-6 w-6 items-center justify-center rounded-full text-xs font-bold ${styles[color] || 'bg-slate-700'}">${escapeHtml(color)}</span>`).join('') : '<span class="text-xs text-slate-500">Color data unavailable</span>';
}

function manaIcon(color) { return `<img class="filter-icon" src="https://svgs.scryfall.io/card-symbols/${encodeURIComponent(color)}.svg" alt="${escapeHtml(color)}">`; }
function typeIcon(type) {
  const key = ['creature','instant','sorcery','land','artifact','enchantment','planeswalker','battle'].find(item => type.toLowerCase().includes(item));
  return key ? `<i class="ms ms-${key}" aria-hidden="true"></i>` : '';
}
function rarityBadge(rarity) {
  const value = rarity || 'Unknown';
  const colors = {'Common':'text-slate-300 bg-slate-400/10','Uncommon':'text-cyan-200 bg-cyan-400/10','Rare':'text-amber-200 bg-amber-400/10','Mythic Rare':'text-orange-200 bg-orange-400/10','Basic Land':'text-emerald-200 bg-emerald-400/10','Special':'text-violet-200 bg-violet-400/10','Unknown':'text-slate-500 bg-white/5'};
  return `<span class="mono rounded px-2 py-1 text-[10px] uppercase tracking-wide ${colors[value] || colors.Unknown}">${escapeHtml(value)}</span>`;
}

// Scryfall resolves an exact MTGA printing when set + collector number are
// known. The named endpoint is a useful fallback for cards without printing
// metadata. Images are lazy-loaded and hidden on error so offline use keeps
// the normal text layout.
function scryfallImageUrl(card, version = 'small', language = 'fr') {
  const aliases = {DAR:'DOM', KLD:'KLR', M19:'M19', ANB:'ANB'};
  const set = String(card.setCode || '').trim().toLowerCase();
  const number = String(card.collectorNumber || '').trim();
  if (set && number && !/^0+$/.test(number)) return `https://api.scryfall.com/cards/${encodeURIComponent(aliases[set.toUpperCase()]?.toLowerCase() || set)}/${encodeURIComponent(number)}/${language}?format=image&version=${version}`;
  if (card.name && !card.name.startsWith('Arena card #')) return `https://api.scryfall.com/cards/named?exact=${encodeURIComponent(card.name)}&lang=${language}&format=image&version=${version}`;
  return '';
}

function imageFallbackUrl(card, version = 'normal') { return scryfallImageUrl(card, version, 'en'); }

function deckCard(deck) {
  const played = (deck.wins || 0) + (deck.losses || 0);
  const winRate = played ? `${Math.round((deck.wins || 0) / played * 100)}%` : '—';
  return `<button data-deck-id="${escapeHtml(deck.id)}" onclick="openDeck(this.dataset.deckId)" class="deck-tile w-full p-5 text-left transition">
    <div class="flex items-start justify-between gap-3"><div><h3 class="text-xl font-bold text-chalk">${escapeHtml(deck.name)}</h3><p class="mono mt-1 text-[11px] text-slate-500">${escapeHtml(deck.format || 'Format non renseigné')} · ${deck.cardCount} cards</p></div><span aria-hidden="true" class="text-gold">↗</span></div>
    ${deck.isUserDeck ? '<span class="mt-3 inline-flex rounded-full bg-emerald-400/10 px-2 py-1 text-[11px] font-semibold text-emerald-300">Création personnelle</span>' : ''}
    <div class="mt-5 flex items-end justify-between gap-3"><div class="flex gap-1.5">${colorPips(deck.colors)}</div><div class="text-right"><p class="mono text-lg font-bold ${played ? 'text-emerald-300' : 'text-slate-500'}">${winRate}</p><p class="mono text-[10px] uppercase tracking-wide text-slate-500">win rate${played ? ` · ${deck.wins}–${deck.losses}` : ''}</p></div></div>
  </button>`;
}

function renderDecks() {
  const decks = state.onlyUserDecks ? state.decks.filter(deck => deck.isUserDeck) : state.decks;
  document.getElementById('view-decks').innerHTML = heading('Library', 'Mes decks', `${decks.length} of ${state.decks.length} deck${state.decks.length === 1 ? '' : 's'} read from Player.log.`) + `
    <div class="plate-soft mb-5 flex items-center justify-between px-4 py-3"><label class="flex cursor-pointer items-center gap-3 text-sm font-medium text-chalk"><input type="checkbox" ${state.onlyUserDecks ? 'checked' : ''} onchange="state.onlyUserDecks = this.checked; renderDecks()" class="h-4 w-4 rounded-sm border-white/20 bg-slate-900 text-gold focus:ring-gold/50">Uniquement mes créations</label><span class="mono text-[11px] text-slate-500">${state.decks.filter(deck => deck.isUserDeck).length} custom</span></div>
    <div class="grid gap-4 md:grid-cols-2 xl:grid-cols-3">${decks.map(deckCard).join('') || empty(state.onlyUserDecks ? 'No decks built by you were detected.' : 'Ouvrez vos decks dans MTGA puis synchronisez.')}</div>`;
}

function groupCards(cards) {
  const groups = { Creatures: [], Spells: [], Lands: [] };
  for (const card of cards) {
    const type = card.typeLine.toLowerCase();
    if (type.includes('creature')) groups.Creatures.push(card);
    else if (type.includes('land')) groups.Lands.push(card);
    else groups.Spells.push(card);
  }
  return groups;
}

function cardRows(cards) {
  return cards.map(card => `<button class="deck-card-row" ${previewAttributes(card)} aria-label="Lire ${escapeHtml(card.name)}"><span class="deck-card-thumb">${cardImage(card, 'small')}</span><span class="copy-count">${card.quantity}×</span><span class="deck-card-name">${escapeHtml(card.name)}</span><span class="card-print">${escapeHtml(card.setCode || '')}</span></button>`).join('');
}

function cardGroups(cards) {
  const groups = groupCards(cards);
  return Object.entries(groups).filter(([, items]) => items.length).map(([name, items]) => `<article class="plate p-5"><h2 class="mb-3 font-bold text-chalk">${name} <span class="mono ml-1 text-xs font-normal text-slate-500">${items.reduce((sum, card) => sum + card.quantity, 0)}</span></h2>${cardRows(items)}</article>`).join('');
}

async function loadManaCurve(deck) {
  const container = document.getElementById('mana-curve');
  if (!container || !deck.mainDeck.length) return;
  const cards = deck.mainDeck;
  const curve = new Map();
  let lands = 0;
  await Promise.all(cards.map(async card => {
    const key = `${card.name}|${card.setCode || ''}|${card.collectorNumber || ''}`;
    let data = state.scryfallCache.get(key);
    if (data === undefined) {
      const set = String(card.setCode || '').toLowerCase();
      const number = String(card.collectorNumber || '').trim();
      const exact = set && number && !/^0+$/.test(number) ? `https://api.scryfall.com/cards/${encodeURIComponent(set)}/${encodeURIComponent(number)}` : `https://api.scryfall.com/cards/named?exact=${encodeURIComponent(card.name)}`;
      data = await fetch(exact).then(response => response.ok ? response.json() : null).catch(() => null);
      state.scryfallCache.set(key, data);
    }
    const type = String(data?.type_line || card.typeLine || '').toLowerCase();
    if (type.includes('land')) { lands += card.quantity; return; }
    if (typeof data?.cmc !== 'number') return;
    const value = Math.min(8, Math.max(0, Math.floor(data.cmc)));
    curve.set(value, (curve.get(value) || 0) + card.quantity);
  }));
  const max = Math.max(1, ...curve.values());
  const bars = [...curve.entries()].sort((a, b) => a[0] - b[0]).map(([value, count]) => `<div class="flex min-w-8 flex-1 flex-col items-center gap-2"><span class="mono text-xs text-slate-400">${count}</span><div class="flex h-32 w-full items-end"><div data-mana-bar class="mana-bar w-full rounded-t bg-gold/80" style="height:${Math.max(4, count / max * 100)}%" title="Mana value ${value === 8 ? '8+' : value}: ${count} cards"></div></div><span class="mono text-xs font-semibold text-slate-400">${value === 8 ? '8+' : value}</span></div>`).join('');
  container.innerHTML = `<div class="flex items-end gap-3">${bars || '<p class="text-sm text-slate-500">Mana values unavailable for this deck.</p>'}</div><p class="mt-4 text-xs text-slate-500">${lands} lands · Data enriched from Scryfall when online.</p>`;
  const manaBars = container.querySelectorAll('[data-mana-bar]');
  if (!reduceMotion && manaBars.length && typeof motionApi.animate === 'function' && typeof motionApi.stagger === 'function') {
    motionApi.animate(manaBars, { scaleY: [0, 1], opacity: [0.35, 1] }, { delay: motionApi.stagger(0.06), duration: 0.48, ease: 'circOut' });
  }
}

function openDeck(id) {
  const deck = state.decks.find(item => item.id === id);
  if (!deck) return;
  state.currentDeckId = id;
  const encodedId = encodeURIComponent(deck.id);
  document.getElementById('view-deck-detail').innerHTML = `
    <button onclick="showView('decks')" class="mb-5 text-sm font-semibold text-slate-400 hover:text-white">Retour aux decks</button>
    <div class="mb-7 flex flex-col justify-between gap-4 sm:flex-row sm:items-end"><div>${heading(deck.format || 'Deck', deck.name, `${deck.cardCount} cards in the main deck.`)}</div>
      <div class="flex flex-wrap gap-2"><button id="analyze-deck-button" data-deck-id="${escapeHtml(deck.id)}" onclick="analyzeDeck(this.dataset.deckId)" class="action-secondary px-4 py-2 text-sm font-bold transition disabled:cursor-wait disabled:opacity-60">Explorer mes combos</button><button data-deck-id="${escapeHtml(deck.id)}" onclick="copyArena(this.dataset.deckId)" class="action-primary px-4 py-2 text-sm font-bold">Copier pour Arena</button>${['json','csv'].map(format => `<a href="/api/decks/${encodedId}/export?format=${format}" class="action-secondary px-4 py-2 text-sm font-semibold uppercase hover:border-gold/40">${format}</a>`).join('')}</div></div>
    <section class="mb-8 grid gap-3 sm:grid-cols-2 xl:grid-cols-5"><article class="rounded-xl border border-white/10 bg-panel/70 p-4"><p class="text-xs uppercase tracking-wider text-slate-500">Record</p><p class="mt-2 text-xl font-bold text-white">${deck.wins}–${deck.losses}${deck.draws ? `–${deck.draws}` : ''}</p><p class="text-xs text-slate-500">wins · losses${deck.draws ? ' · draws' : ''}</p></article><article class="rounded-xl border border-white/10 bg-panel/70 p-4"><p class="text-xs uppercase tracking-wider text-slate-500">Last played</p><p class="mt-2 text-sm font-semibold text-white">${escapeHtml(formatDeckDate(deck.lastPlayed))}</p></article><article class="rounded-xl border border-white/10 bg-panel/70 p-4"><p class="text-xs uppercase tracking-wider text-slate-500">Last updated</p><p class="mt-2 text-sm font-semibold text-white">${escapeHtml(formatDeckDate(deck.lastUpdated))}</p></article><article class="rounded-xl border border-white/10 bg-panel/70 p-4"><p class="text-xs uppercase tracking-wider text-slate-500">Status</p><p class="mt-2 text-sm font-semibold ${deck.isFavorite ? 'text-gold' : 'text-slate-300'}">${deck.isFavorite ? '★ Favorite' : 'Regular deck'}</p></article><article class="rounded-xl border border-white/10 bg-panel/70 p-4"><p class="text-xs uppercase tracking-wider text-slate-500">Queues</p><p class="mt-2 text-sm font-semibold text-white">${deck.events?.length ? escapeHtml(deck.events.join(', ')) : 'Non renseigné'}</p></article></section>
    <section class="mb-8 rounded-2xl border border-white/10 bg-panel/80 p-5"><div class="flex items-center justify-between"><div><h2 class="font-bold text-white">Courbe de mana</h2><p class="mt-1 text-xs text-slate-500">Répartition des sorts par valeur de mana, hors terrains.</p></div></div><div id="mana-curve" class="mt-5"><div class="h-32 animate-pulse rounded bg-white/5" aria-hidden="true"></div></div></section>
    <section class="mb-8 grid gap-4 xl:grid-cols-[minmax(0,1fr)_18rem]">
      <div id="analysis-report" class="min-h-28 rounded-2xl border border-violet-400/20 bg-violet-400/5 p-6"><p class="text-sm text-slate-400">Découvrez les synergies de votre deck et les assemblages possibles avec vos cartes. Cliquez sur « Explorer mes combos » pour envoyer le deck et une sélection de votre collection à Gemini. Le rapport sera conservé ici.</p></div>
      <aside class="rounded-2xl border border-white/10 bg-panel/80 p-5"><h2 class="font-bold text-white">Carnet stratégique</h2><div id="analysis-history" class="mt-3"><span class="block h-3 w-32 animate-pulse rounded bg-white/10" aria-hidden="true"></span><span class="mt-2 block h-3 w-24 animate-pulse rounded bg-white/10" aria-hidden="true"></span></div></aside>
    </section>
    <div class="grid gap-4 lg:grid-cols-2">${cardGroups(deck.mainDeck)}${deck.commandZone.length ? `<article class="rounded-2xl border border-gold/20 bg-panel/80 p-5"><h2 class="mb-3 font-bold text-white">Commander</h2>${cardRows(deck.commandZone)}</article>` : ''}${deck.sideboard.length ? `<article class="rounded-2xl border border-white/10 bg-panel/80 p-5"><h2 class="mb-3 font-bold text-white">Sideboard</h2>${cardRows(deck.sideboard)}</article>` : ''}</div>`;
  showView('deck-detail');
  loadManaCurve(deck).catch(error => {
    const curve = document.getElementById('mana-curve');
    if (curve) curve.innerHTML = `<p class="text-sm text-slate-500">Courbe de mana unavailable: ${escapeHtml(error.message)}</p>`;
  });
  loadAnalysisHistory(deck.id);
}

async function analyzeDeck(deckId) {
  const button = document.getElementById('analyze-deck-button');
  tapFeedback(button);
  if (button) { button.disabled = true; button.textContent = 'Exploration en cours…'; }
  document.getElementById('analysis-report').innerHTML = `<div class="flex items-center gap-3 text-violet-200"><span class="h-5 w-5 animate-spin rounded-full border-2 border-violet-300 border-t-transparent"></span><span>Gemini explore les synergies de vos cartes…</span></div>`;
  try {
    const report = await api('/api/analyze-deck', {
      method: 'POST',
      headers: {'content-type':'application/json'},
      body: JSON.stringify({deckId})
    });
    if (state.currentDeckId !== deckId || !button?.isConnected) return;
    renderAnalysis(report);
    await loadAnalysisHistory(deckId);
    showAlert('Analyse enregistrée dans votre carnet.');
  } catch (error) {
    if (state.currentDeckId !== deckId || !button?.isConnected) return;
    document.getElementById('analysis-report').innerHTML = `<p class="font-semibold text-red-200">Analysis unavailable</p><p class="mt-2 text-sm text-red-200/70">${escapeHtml(error.message)}</p>`;
  } finally {
    if (button && state.currentDeckId === deckId) { button.disabled = false; button.textContent = 'Explorer mes combos'; }
  }
}

async function loadAnalysisHistory(deckId) {
  const history = document.getElementById('analysis-history');
  try {
    const reports = await api(`/api/decks/${encodeURIComponent(deckId)}/analyses`);
    if (state.currentDeckId !== deckId || !history) return;
    history.innerHTML = reports.length ? reports.map(report => `<button data-report-id="${report.id}" onclick="loadAnalysis(this.dataset.reportId)" class="mb-2 w-full rounded-xl border border-white/10 px-3 py-3 text-left transition hover:border-violet-400/40"><span class="block font-semibold text-slate-200">${escapeHtml(new Date(report.createdAt * 1000).toLocaleString())}</span><span class="mt-1 block text-xs text-slate-500">${escapeHtml(report.model)} · ${report.collectionCardCount} cards</span></button>`).join('') : 'Vos analyses apparaîtront ici.';
  } catch (error) {
    if (history) history.textContent = error.message;
  }
}

let analysisLoadVersion = 0;
async function loadAnalysis(reportId) {
  const version = ++analysisLoadVersion;
  const panel = document.getElementById('analysis-report');
  panel.innerHTML = '<div class="space-y-3" aria-label="Analysis loading"><span class="block h-4 w-2/3 animate-pulse rounded bg-white/10"></span><span class="block h-3 w-full animate-pulse rounded bg-white/10"></span><span class="block h-3 w-5/6 animate-pulse rounded bg-white/10"></span></div>';
  try { const report = await api(`/api/analyses/${encodeURIComponent(reportId)}`); if (version === analysisLoadVersion && panel.isConnected && report.deckId === state.currentDeckId) renderAnalysis(report); }
  catch (error) { if (version !== analysisLoadVersion || !panel.isConnected) return; panel.innerHTML = `<p class="text-red-200">${escapeHtml(error.message)}</p>`; }
}

async function copyArena(id) {
  try {
    const response = await fetch(`/api/decks/${encodeURIComponent(id)}/export?format=arena`);
    if (!response.ok) throw new Error('Export failed.');
    await navigator.clipboard.writeText(await response.text());
    showAlert('Arena deck copied to the clipboard.');
  } catch (error) { showAlert(error.message, true); }
}

async function renderSettings() {
  const settings = await api('/api/settings');
  document.getElementById('view-settings').innerHTML = heading('Configuration', 'Réglages', 'Point Magic Deck at the Player.log generated by your Steam/Proton installation.') + `
    <form onsubmit="saveSettings(event)" class="max-w-3xl rounded-2xl border border-white/10 bg-panel/80 p-6"><label for="log-path" class="text-sm font-semibold text-white">MTGA Player.log path</label><input id="log-path" value="${escapeHtml(settings.logPath)}" class="mt-3 w-full rounded-xl border border-white/10 bg-slate-950 px-4 py-3 font-mono text-sm outline-none focus:border-gold/50"><p class="mt-3 text-xs text-slate-500">You may use an absolute path or <code>~/…</code>. The setting is stored in ~/.config/magic-deck/config.json.</p><label for="gemini-key" class="mt-6 block text-sm font-semibold text-white">Gemini API key</label><input id="gemini-key" type="password" autocomplete="new-password" placeholder="${settings.geminiConfigured ? 'Configured — enter a new key to replace it' : 'Paste your Gemini API key'}" class="mt-3 w-full rounded-xl border border-white/10 bg-slate-950 px-4 py-3 font-mono text-sm outline-none focus:border-gold/50"><p class="mt-3 text-xs text-slate-500">Stored locally with restricted permissions. The key is never displayed or sent to the browser.</p><div class="mt-6 flex gap-3"><button class="rounded-xl bg-gold px-5 py-3 text-sm font-bold text-slate-950">Save</button><button data-sync="true" class="rounded-xl border border-white/10 px-5 py-3 text-sm font-semibold">Save, then sync</button></div></form>`;
}

async function saveSettings(event) {
  event.preventDefault();
  try {
    const payload = {logPath: document.getElementById('log-path').value};
    const apiKey = document.getElementById('gemini-key').value.trim();
    if (apiKey) payload.geminiApiKey = apiKey;
    await api('/api/settings', { method: 'PUT', headers: {'content-type':'application/json'}, body: JSON.stringify(payload) });
    showAlert('Réglages enregistrés.');
    await loadStatus();
    if (event.submitter?.dataset.sync) await syncData();
  } catch (error) { showAlert(error.message, true); }
}

function empty(message) { return `<div class="col-span-full rounded-2xl border border-dashed border-white/10 p-10 text-center text-slate-500">${escapeHtml(message)}</div>`; }

async function loadStatus() {
  [state.status, state.decks, state.collection] = await Promise.all([api('/api/status'), api('/api/decks'), api('/api/collection')]);
  document.getElementById('sidebar-status').textContent = `Dernière synchro: ${formatTime(state.status.syncedAt)}`;
  renderHome(); renderDecks(); renderCollection();
  animateView(document.getElementById(`view-${state.currentView}`));
}

async function syncData() {
  const buttons = [document.getElementById('sync-desktop'), document.getElementById('sync-mobile')];
  buttons.forEach(tapFeedback);
  buttons.forEach(button => { if (button) { button.disabled = true; button.textContent = 'Synchronisation…'; } });
  try {
    await api('/api/sync', {method:'POST'});
    await loadStatus();
    showAlert('Données MTGA synchronisées.');
  } catch (error) { showAlert(error.message, true); }
  finally { buttons.forEach(button => { if (button) { button.disabled = false; button.textContent = button.id === 'sync-mobile' ? 'Sync' : 'Synchroniser'; } }); }
}

const originalShowView = showView;
showView = function(view) { originalShowView(view); if (view === 'settings') renderSettings().catch(error => showAlert(error.message, true)); };
window.addEventListener('DOMContentLoaded', () => loadStatus().catch(error => showAlert(error.message, true)));
