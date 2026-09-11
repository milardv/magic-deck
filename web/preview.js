// One accessible viewer shared by the collection, deck and coaching reports.
// Native dialog provides focus confinement and restores focus on close.
const cardDialog = document.getElementById('card-dialog');
let viewerGeneration = 0;
let viewerCloseTimer;
function showCardFocus(trigger) {
  if (!trigger.dataset.cardImage) return;
  const generation = ++viewerGeneration;
  clearTimeout(viewerCloseTimer);
  cardDialog.classList.remove('closing');
  const image = document.getElementById('viewer-image');
  const message = document.getElementById('viewer-message');
  image.hidden = true;
  message.textContent = '';
  document.getElementById('viewer-name').textContent = trigger.dataset.cardName || '';
  image.alt = trigger.dataset.cardName || '';
  let fallback = trigger.dataset.cardFallback;
  image.onload = () => { if (generation === viewerGeneration) image.hidden = false; };
  image.onerror = () => {
    if (generation !== viewerGeneration) return;
    if (fallback && image.src !== fallback) { const next = fallback; fallback = ''; image.src = next; }
    else { image.hidden = true; message.textContent = 'Illustration indisponible. Vous pouvez toujours consulter les informations de la carte.'; }
  };
  image.src = trigger.dataset.cardImage;
  if (!cardDialog.open) cardDialog.showModal();
}
function hideCardFocus() {
  if (!cardDialog.open) return;
  clearTimeout(viewerCloseTimer);
  ++viewerGeneration;
  cardDialog.classList.add('closing');
  const delay = matchMedia('(prefers-reduced-motion: reduce)').matches ? 0 : 180;
  viewerCloseTimer = setTimeout(() => { cardDialog.close(); cardDialog.classList.remove('closing'); }, delay);
}
cardDialog.addEventListener('cancel', event => { event.preventDefault(); hideCardFocus(); });
cardDialog.addEventListener('click', event => { if (event.target === cardDialog) hideCardFocus(); });
document.addEventListener('click', event => {
  const trigger = event.target.closest?.('[data-card-image]');
  if (trigger) showCardFocus(trigger);
});
function previewAttributes(card) {
  return `data-card-image="${escapeHtml(scryfallImageUrl(card, 'large'))}" data-card-fallback="${escapeHtml(imageFallbackUrl(card, 'large'))}" data-card-name="${escapeHtml(card.name)}"`;
}
function cardImage(card, version = 'normal') {
  const image = scryfallImageUrl(card, version);
  return image ? `<img loading="lazy" decoding="async" src="${escapeHtml(image)}" alt="${escapeHtml(card.name)}" data-fallback="${escapeHtml(imageFallbackUrl(card, version))}" onerror="handleCardImageError(this)"><span class="art-unavailable" hidden>Illustration indisponible</span>` : '<span class="art-unavailable">Illustration indisponible</span>';
}
function handleCardImageError(image) {
  const fallback = image.dataset.fallback;
  delete image.dataset.fallback;
  if (fallback && image.src !== fallback) image.src = fallback;
  else { image.hidden = true; if (image.nextElementSibling) image.nextElementSibling.hidden = false; }
}
