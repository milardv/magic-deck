// Run against a local Magic Deck server. API fixtures isolate the real MTGA
// account and never send a paid Gemini request. Screenshots are opt-in.
const { chromium } = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
const base = process.env.MAGIC_DECK_TEST_URL || 'http://127.0.0.1:18092';
const cards = Array.from({ length: 45 }, (_, i) => ({ arenaId: i + 1, name: ['Pacifism', 'Lyra Dawnbringer', 'Island'][i % 3] + (i > 2 ? ` ${i}` : ''), quantity: i % 4 + 1, colors: i % 3 === 2 ? [] : ['W'], rarity: ['Common', 'Mythic Rare', 'Basic Land'][i % 3], typeLine: ['Enchantment', 'Creature — Angel', 'Basic Land — Island'][i % 3], setCode: ['M20', 'FDN', 'M20'][i % 3], collectorNumber: ['32', '707', '265'][i % 3] }));
const deck = { id:'test-deck', name:'Les ailes de l’aube', format:'Historic', mainDeck:cards.slice(0,3), sideboard:[], commandZone:[], colors:['W','U'], cardCount:60, wins:7, losses:4, draws:0, events:[], isUserDeck:true };
const report = { id:1, deckId:deck.id, deckName:deck.name, createdAt:1789000000, model:'test-fixture', deckCardCount:60, collectionCardCount:110, analysis:{
  deck_summary:'Exemple de rapport pour tester l’interface, pas un conseil de jeu.', game_plan:'Développer le plateau tout en gardant une réponse disponible.', strengths:['Un plan de jeu lisible'], weaknesses:['Une base de mana à examiner'], improvement_suggestions:[], play_challenge:'Observez la différence entre développer le plateau et garder une interaction.',
  combos:[{ title:'Votre terrain d’expérimentation', kind:'synergy', cards:[{card_name:'Pacifism',quantity:1},{card_name:'Lyra Dawnbringer',quantity:1}], prerequisites:'Exemple de données de test uniquement.', steps:['Première étape de démonstration.', 'Deuxième étape de démonstration.', 'Dernière étape de démonstration.'], payoff:'Un résultat affiché clairement.', limitations:'Cette fixture ne prétend pas valider une interaction de règles.' }]
} };
(async () => {
  const browser = await chromium.launch({ headless:true });
  try {
    for (const [name, viewport, reducedMotion] of [['desktop',{width:1440,height:1000},'no-preference'],['mobile',{width:390,height:844},'reduce']]) {
      const context = await browser.newContext({viewport, reducedMotion});
      // Prove the preview remains usable without the animation CDN.
      await context.route('**/motion@*/**', route => route.abort());
      await context.route(`${base}/api/**`, route => {
        const path = new URL(route.request().url()).pathname;
        const payload = path === '/api/decks' ? [deck] : path === '/api/collection' ? cards : path === '/api/status' ? { deckCount:1, cardsOwned:45, totalCopies:110, wildcards:{rare:3,mythic:2}, warnings:[], logExists:true } : path.endsWith('/analyses') ? [report] : path === '/api/analyze-deck' || path === '/api/analyses/1' ? report : {};
        return route.fulfill({json:payload});
      });
      const page = await context.newPage(); const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      await page.goto(base);
      await page.locator('[data-nav=collection]').click();
      await page.locator('.collection-art').first().waitFor();
      assert.equal(await page.locator('.collection-card').count(),36);
      const search = page.locator('#collection-search'); await search.pressSequentially('Lyra');
      assert.equal(await search.inputValue(),'Lyra'); assert.equal(await search.evaluate(e => document.activeElement === e),true);
      await search.fill('');
      if (name === 'mobile') await page.locator('.advanced-filters summary').click();
      await page.locator('[data-color=C]').click(); assert.equal(await page.locator('.collection-card').count(),15);
      await page.getByRole('button',{name:'Réinitialiser',exact:true}).click();
      await page.locator('#collection-rarity').selectOption('Mythic Rare'); assert.equal(await page.locator('.collection-card').count(),15);
      await page.getByRole('button',{name:'Réinitialiser',exact:true}).click();
      await page.locator('.favorite-button').first().click(); await page.locator('#favorites-filter').click(); assert.equal(await page.locator('.collection-card').count(),1);
      await page.reload(); await page.locator('[data-nav=collection]').click(); await page.locator('#favorites-filter').click(); assert.equal(await page.locator('.collection-card').count(),1);
      await page.locator('#favorites-filter').click(); await page.locator('#gallery-filter').click();
      await page.locator('.collection-art').first().hover(); assert.equal(await page.locator('#card-dialog').evaluate(e => e.open),false);
      await page.locator('.collection-art').first().click(); assert.equal(await page.locator('#card-dialog').evaluate(e => e.open),true);
      await page.keyboard.press('Escape'); await page.waitForFunction(() => !document.getElementById('card-dialog').open);
      assert.equal(await page.locator('.collection-art').first().evaluate(e => document.activeElement === e),true);
      await page.locator('#gallery-filter').click();
      await page.locator('.collection-art img').first().waitFor();
      await page.waitForFunction(() => { const i = document.querySelector('.collection-art img'); return i.complete && i.naturalWidth > 0; });
      if (process.env.SCREENSHOT_DIR) await page.screenshot({path:`${process.env.SCREENSHOT_DIR}/collection-${name}.png`,fullPage:false});
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'collection must not overflow horizontally');
      await page.locator('[data-nav=decks]').click(); await page.locator('#view-decks .deck-tile').first().click();
      await page.locator('#analyze-deck-button').click(); await page.locator('.combo-workbench').waitFor();
      await page.locator('.step-next').click(); assert.equal(await page.locator('.step-text').textContent(),report.analysis.combos[0].steps[1]);
      await page.locator('.step-next').click(); assert.equal(await page.locator('.step-next').isDisabled(),true);
      await page.locator('.step-back').click();
      await page.locator('.combo-card').first().click(); await page.getByRole('button',{name:'Fermer l’aperçu'}).click(); await page.waitForFunction(() => !document.getElementById('card-dialog').open);
      await page.locator('[data-coach-tab=plan]').click(); assert.equal(await page.locator('[data-coach-section=plan]').isVisible(),true);
      await page.locator('[data-coach-tab=combos]').click();
      await page.locator('#analysis-report').scrollIntoViewIfNeeded();
      if (process.env.SCREENSHOT_DIR) await page.screenshot({path:`${process.env.SCREENSHOT_DIR}/coach-${name}.png`,fullPage:false});
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), 'deck must not overflow horizontally');
      const legacy = structuredClone(report); delete legacy.analysis.combos; delete legacy.analysis.play_challenge;
      await page.evaluate(r => renderAnalysis(r),legacy); assert.equal(await page.locator('.combo-workbench').count(),0);
      assert.deepEqual(errors,[]);
      console.log(`${name}: collection, filters, persisted favorites, viewer, reduced motion/CDN fallback, combos and legacy report OK`);
      await context.close();
    }
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode=1; });
