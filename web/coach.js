// Reports remain immutable; stepping through a combo only changes its presentation.
let displayedReport = null;
function renderAnalysis(report) {
  displayedReport = report;
  const a = report.analysis;
  const combos = a.combos || [];
  const panel = document.getElementById('analysis-report');
  panel.innerHTML = `<div class="coach-heading"><div><h2>Atelier stratégique</h2><p>${escapeHtml(report.deckName)}</p></div><span class="report-date">${escapeHtml(new Date(report.createdAt * 1000).toLocaleString('fr-FR'))}${report.cached ? ' · Rapport sauvegardé' : ''}</span></div>
    <p class="coach-summary">${escapeHtml(a.deck_summary)}</p>
    <div class="coach-tabs" role="group" aria-label="Sections du rapport">
      <button class="filter-chip" data-coach-tab="combos" aria-pressed="true" onclick="coachTab('combos')">Combos & synergies <span>${combos.length}</span></button>
      <button class="filter-chip" data-coach-tab="plan" aria-pressed="false" onclick="coachTab('plan')">Plan de jeu</button>
      <button class="filter-chip" data-coach-tab="changes" aria-pressed="false" onclick="coachTab('changes')">Pistes de construction</button>
    </div>
    <div data-coach-section="combos">
      <p class="coach-note">Pistes proposées par l’IA, à vérifier avec le texte des cartes. Une synergie n’est pas forcément une boucle infinie.</p>
      ${combos.map(renderCombo).join('') || '<div class="collection-empty"><h3>Aucune combinaison détaillée dans ce rapport</h3><p>Consultez le plan de jeu. Si ce rapport est ancien, une nouvelle analyse utilisera le nouvel atelier de combos.</p></div>'}
    </div>
    <div data-coach-section="plan" hidden><h3>Votre ligne directrice</h3><p class="coach-prose">${escapeHtml(a.game_plan)}</p>
      <div class="strategy-columns"><section><h3>Vos atouts</h3><ul>${a.strengths.map(s => `<li>${escapeHtml(s)}</li>`).join('')}</ul></section><section><h3>À surveiller</h3><ul>${a.weaknesses.map(s => `<li>${escapeHtml(s)}</li>`).join('')}</ul></section></div>
    </div>
    <div data-coach-section="changes" hidden><p class="coach-note">Chaque proposition est une expérience indépendante. Les changements ne sont pas appliqués à votre deck.</p>${a.improvement_suggestions.map(renderDeckChange).join('') || '<p>Aucun changement recommandé.</p>'}</div>
    ${a.play_challenge ? `<section class="play-challenge"><h3>À tenter lors de votre prochaine partie</h3><p>${escapeHtml(a.play_challenge)}</p></section>` : ''}
    <footer class="report-footer"><span>Snapshot : ${report.deckCardCount} cartes de deck · ${report.collectionCardCount} exemplaires possédés</span><button class="text-button" onclick="downloadCoachReport()">Télécharger le rapport</button></footer>`;
}
function reportCard(name) {
  // Image lookup is presentation only. Availability always comes from server validation
  // against the report snapshot, never inferred from today's collection.
  const cards = state.decks.flatMap(deck => [...deck.mainDeck, ...deck.sideboard, ...deck.commandZone]);
  return [...cards, ...state.collection].find(card => card.name === name) || { name };
}
function illustratedCard(change) {
  const card = reportCard(change.card_name);
  return `<button class="combo-card" ${previewAttributes(card)} aria-label="Lire ${escapeHtml(card.name)}"><span class="combo-art">${cardImage(card, 'normal')}</span><span>${change.quantity}× ${escapeHtml(card.name)}</span></button>`;
}
function renderCombo(combo, index) {
  const kinds = { synergy: 'Synergie', sequence: 'Enchaînement', repeatable_loop: 'Boucle conditionnelle', infinite_loop: 'Boucle infinie proposée' };
  return `<section class="combo-workbench"><div class="combo-title"><h3>${escapeHtml(combo.title)}</h3><span class="combo-kind">${kinds[combo.kind] || 'Interaction'}</span></div>
    <div class="combo-cards">${combo.cards.map(illustratedCard).join('')}</div>
    <p class="combo-prerequisites"><strong>Avant de commencer</strong> ${escapeHtml(combo.prerequisites)}</p>
    <div class="combo-sequence" data-combo="${index}" data-step="0"><div class="step-track" role="group" aria-label="Étapes de la combinaison">${combo.steps.map((_, step) => `<button class="step-button" aria-label="Étape ${step + 1}" aria-pressed="${step === 0}" onclick="setComboStep(${index}, ${step})">${step + 1}</button>`).join('')}</div>
    <p class="step-text" aria-live="polite">${escapeHtml(combo.steps[0] || '')}</p>
    <div class="step-actions"><button class="text-button step-back" disabled onclick="advanceCombo(${index}, -1)">Précédente</button><span class="step-count">1 / ${combo.steps.length}</span><button class="text-button step-next" ${combo.steps.length <= 1 ? 'disabled' : ''} onclick="advanceCombo(${index}, 1)">Étape suivante</button></div></div>
    <p class="combo-payoff"><strong>Ce que vous gagnez</strong> ${escapeHtml(combo.payoff)}</p>
    <details class="combo-limits"><summary>Les limites et les moyens de l’interrompre</summary><p>${escapeHtml(combo.limitations)}</p></details></section>`;
}
function coachTab(name) {
  document.querySelectorAll('[data-coach-tab]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.coachTab === name)));
  document.querySelectorAll('[data-coach-section]').forEach(section => { section.hidden = section.dataset.coachSection !== name; });
}
function advanceCombo(index, direction) { const node = document.querySelector(`[data-combo="${index}"]`); setComboStep(index, Number(node.dataset.step) + direction); }
function setComboStep(index, step) {
  const combo = displayedReport?.analysis.combos?.[index];
  const node = document.querySelector(`[data-combo="${index}"]`);
  if (!combo || !node || step < 0 || step >= combo.steps.length) return;
  node.dataset.step = step;
  const text = node.querySelector('.step-text');
  text.textContent = combo.steps[step];
  node.querySelectorAll('.step-button').forEach((button, i) => button.setAttribute('aria-pressed', String(i === step)));
  node.querySelector('.step-back').disabled = step === 0;
  node.querySelector('.step-next').disabled = step === combo.steps.length - 1;
  node.querySelector('.step-count').textContent = `${step + 1} / ${combo.steps.length}`;
  if (!matchMedia('(prefers-reduced-motion: reduce)').matches) text.animate([{ opacity: .4 }, { opacity: 1 }], { duration: 180 });
}
function renderDeckChange(change) {
  const priority = { high: 'Prioritaire', medium: 'À essayer', low: 'Pour explorer' };
  return `<section class="deck-change"><div class="combo-title"><h3>${escapeHtml(change.title)}</h3><span class="combo-kind">${priority[change.priority] || ''}</span></div><div class="strategy-columns"><div><h4>Retirer</h4>${change.card_to_remove.map(c => `<p>${c.quantity}× ${escapeHtml(c.card_name)}</p>`).join('') || '<p>Aucun retrait</p>'}</div><div><h4>Ajouter</h4>${change.card_to_add.map(c => `<p>${c.quantity}× ${escapeHtml(c.card_name)}</p>`).join('') || '<p>Aucun ajout</p>'}</div></div><p class="coach-prose">${escapeHtml(change.reasoning)}</p></section>`;
}
function downloadCoachReport() {
  if (!displayedReport) return;
  const url = URL.createObjectURL(new Blob([JSON.stringify(displayedReport, null, 2)], { type: 'application/json' }));
  const link = document.createElement('a'); link.href = url; link.download = `magic-deck-analyse-${displayedReport.id}.json`; link.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
