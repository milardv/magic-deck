// Filters live outside the DOM so typing never replaces the focused input.
const collectionFilters = { search: '', color: '', type: '', rarity: '', sort: 'name', favorites: false, gallery: false, page: 1 };
let favoriteCards = new Set();
try { const saved = JSON.parse(localStorage.getItem('magic-deck.favorites.v1') || '[]'); if (Array.isArray(saved)) favoriteCards = new Set(saved.map(String)); } catch { /* Unavailable storage must not block collection access. */ }
const rarityLabels = { 'Common': 'Commune', 'Uncommon': 'Peu commune', 'Rare': 'Rare', 'Mythic Rare': 'Mythique', 'Basic Land': 'Terrain de base', 'Special': 'Spéciale' };
const cardTypes = { creature: 'Créature', instant: 'Éphémère', sorcery: 'Rituel', enchantment: 'Enchantement', artifact: 'Artefact', land: 'Terrain', planeswalker: 'Planeswalker', battle: 'Bataille' };
const colorNames = { W: 'Blanc', U: 'Bleu', B: 'Noir', R: 'Rouge', G: 'Vert', C: 'Incolore' };
function heartIcon() { return '<svg viewBox="0 0 24 24" width="19" height="19" aria-hidden="true"><path d="M12 20 4 12C-1 5 8 1 12 7c4-6 13-2 8 5Z" fill="none" stroke="currentColor" stroke-width="1.6"/></svg>'; }
function filterCollectionCards(cards, filters, favorites) {
  const query = filters.search.trim().toLocaleLowerCase();
  const rarityOrder = { 'Mythic Rare': 5, Rare: 4, Uncommon: 3, Common: 2, 'Basic Land': 1 };
  return cards.filter(card => (!query || `${card.name} ${card.typeLine} ${card.setCode || ''} ${card.arenaId}`.toLocaleLowerCase().includes(query))
    && (!filters.color || (filters.color === 'C' ? !card.colors.length : card.colors.includes(filters.color)))
    && (!filters.type || card.typeLine.toLowerCase().includes(filters.type))
    && (!filters.rarity || (filters.rarity === 'unknown' ? !card.rarity : card.rarity === filters.rarity))
    && (!filters.favorites || favorites.has(String(card.arenaId))))
    .sort((a, b) => (filters.sort === 'rarity' ? (rarityOrder[b.rarity] || 0) - (rarityOrder[a.rarity] || 0) : filters.sort === 'quantity' ? b.quantity - a.quantity : 0) || a.name.localeCompare(b.name, 'fr') || a.arenaId - b.arenaId);
}
function renderCollection() {
  const view = document.getElementById('view-collection');
  if (!document.getElementById('collection-results')) {
    view.innerHTML = heading('', 'Votre collection', 'Les illustrations qui vous inspirent. Les cartes qui feront votre prochain deck.') + `
      <div class="collection-toolbar">
        <div class="collection-search-row"><label class="search-field">Rechercher<input id="collection-search" type="search" placeholder="Nom, type, extension…" oninput="setCollectionFilter('search', this.value)"></label>
        <button id="favorites-filter" class="filter-chip" aria-pressed="false" onclick="setCollectionFilter('favorites', !collectionFilters.favorites)">${heartIcon()} Mes coups de cœur</button>
        <button id="gallery-filter" class="filter-chip" aria-pressed="false" onclick="setCollectionFilter('gallery', !collectionFilters.gallery)">Mode galerie</button></div>
        <details class="advanced-filters" ${matchMedia('(min-width: 768px)').matches ? 'open' : ''}><summary>Couleurs, types, raretés et tri</summary><div class="collection-filter-row"><div class="color-filters" role="group" aria-label="Couleurs"><button class="filter-chip" data-color="" onclick="setCollectionFilter('color', '')">Toutes</button>${Object.entries(colorNames).map(([code, label]) => `<button class="color-filter" data-color="${code}" aria-label="${label}" title="${label}" onclick="setCollectionFilter('color', '${code}')">${manaIcon(code)}</button>`).join('')}</div>
        <label>Type<select id="collection-type" onchange="setCollectionFilter('type', this.value)"><option value="">Tous les types</option>${Object.entries(cardTypes).map(([code, label]) => `<option value="${code}">${label}</option>`).join('')}</select></label>
        <label>Rareté<select id="collection-rarity" onchange="setCollectionFilter('rarity', this.value)"><option value="">Toutes les raretés</option>${Object.entries(rarityLabels).map(([value, label]) => `<option value="${value}">${label}</option>`).join('')}<option value="unknown">Non renseignée</option></select></label>
        <label>Trier par<select id="collection-sort" onchange="setCollectionFilter('sort', this.value)"><option value="name">Nom</option><option value="rarity">Rareté décroissante</option><option value="quantity">Quantité possédée</option></select></label>
        <button class="text-button" onclick="resetCollectionFilters()">Réinitialiser</button></div></details>
      </div><div class="collection-summary"><p id="collection-count" role="status"></p><div><a href="/api/collection/export?format=csv">Exporter tout · CSV</a><a href="/api/collection/export?format=json">JSON</a></div></div>
      <div id="collection-results" class="collection-grid"></div><nav id="collection-pages" class="pagination" aria-label="Pages de la collection"></nav>`;
  }
  renderCollectionResults();
}
function setCollectionFilter(key, value) { collectionFilters[key] = value; collectionFilters.page = 1; renderCollectionResults(); }
function resetCollectionFilters() {
  Object.assign(collectionFilters, { search: '', color: '', type: '', rarity: '', favorites: false, sort: 'name', page: 1 });
  ['search', 'type', 'rarity', 'sort'].forEach(key => { document.getElementById(`collection-${key}`).value = collectionFilters[key]; });
  renderCollectionResults();
}
function toggleCardFavorite(id) {
  id = String(id);
  if (favoriteCards.has(id)) favoriteCards.delete(id); else favoriteCards.add(id);
  try { localStorage.setItem('magic-deck.favorites.v1', JSON.stringify([...favoriteCards])); }
  catch { showAlert('Favoris conservés pour cette session uniquement : stockage du navigateur indisponible.', true); }
  renderCollectionResults();
  document.querySelector(`[data-favorite-id="${id}"]`)?.focus({ preventScroll: true });
}
function collectionPage(page) { collectionFilters.page = page; renderCollectionResults(); document.getElementById('collection-results').scrollIntoView({ block: 'start' }); }
function renderCollectionResults() {
  const grid = document.getElementById('collection-results');
  if (!grid) return;
  const cards = filterCollectionCards(state.collection, collectionFilters, favoriteCards);
  const pages = Math.max(1, Math.ceil(cards.length / 36));
  collectionFilters.page = Math.min(collectionFilters.page, pages);
  document.getElementById('collection-count').textContent = `${cards.length} cartes différentes · ${cards.reduce((n, c) => n + c.quantity, 0)} exemplaires`;
  document.querySelectorAll('[data-color]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.color === collectionFilters.color)));
  document.getElementById('favorites-filter').setAttribute('aria-pressed', String(collectionFilters.favorites));
  document.getElementById('gallery-filter').setAttribute('aria-pressed', String(collectionFilters.gallery));
  grid.classList.toggle('gallery-mode', collectionFilters.gallery);
  grid.innerHTML = cards.slice((collectionFilters.page - 1) * 36, collectionFilters.page * 36).map(card => `<article class="collection-card">
    <button class="collection-art" ${previewAttributes(card)} aria-label="Agrandir ${escapeHtml(card.name)}">${cardImage(card)}</button>
    <button class="favorite-button" data-favorite-id="${card.arenaId}" aria-pressed="${favoriteCards.has(String(card.arenaId))}" aria-label="Coup de cœur : ${escapeHtml(card.name)}" onclick="toggleCardFavorite('${card.arenaId}')">${heartIcon()}</button>
    <div class="collection-caption"><div class="card-title-row"><h3>${escapeHtml(card.name)}</h3><span class="copy-count">${card.quantity}×</span></div>
    <p class="card-type">${typeIcon(card.typeLine)} ${escapeHtml(card.typeLine)}</p>
    <div class="card-meta"><div class="mana-pips">${(card.colors.length ? card.colors : ['C']).map(manaIcon).join('')}</div><span class="rarity-label" data-rarity="${escapeHtml(card.rarity || '')}">${escapeHtml(rarityLabels[card.rarity] || 'Non renseignée')}</span></div>
    <p class="card-print">${escapeHtml(card.setCode || 'Extension inconnue')} · ${escapeHtml(card.collectorNumber || card.arenaId)}</p></div></article>`).join('') || `<div class="collection-empty"><h2>${collectionFilters.favorites ? 'Votre galerie personnelle commence ici' : 'Aucune carte à afficher'}</h2><p>${state.collection.length ? 'Changez les filtres ou ajoutez des cartes à vos coups de cœur avec le bouton cœur.' : 'Ouvrez MTGA puis synchronisez votre collection pour découvrir vos cartes.'}</p><button class="text-button" onclick="resetCollectionFilters()">Réinitialiser les filtres</button></div>`;
  document.getElementById('collection-pages').innerHTML = `<button class="filter-chip" ${collectionFilters.page === 1 ? 'disabled' : ''} onclick="collectionPage(${collectionFilters.page - 1})">Précédente</button><span>Page ${collectionFilters.page} / ${pages}</span><button class="filter-chip" ${collectionFilters.page === pages ? 'disabled' : ''} onclick="collectionPage(${collectionFilters.page + 1})">Suivante</button>`;
}
