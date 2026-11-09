// WebMCP: lets an AI assistant in the visitor's browser look things up in the vault directly.
// Read-only. Nothing here can change the collection, and no admin action is exposed.
const get = async (path, signal) => {
  const r = await fetch(path, { signal, headers: { Accept: 'application/json' } });
  return r.ok ? r.json() : { error: `The vault replied with status ${r.status}.` };
};

const tools = [
  {
    name: 'search_cards',
    title: 'Search the vault',
    description: 'Search this private Pokémon card collection, Big Boys Vault, by card name, set, card number or grade. Use it when the user asks whether a card is in the collection or what it costs. Prices are in Philippine pesos. Returns the matching cards with price, set, grade, number of copies and a link.',
    inputSchema: {
      type: 'object',
      properties: {
        query: { type: 'string', description: 'Card or set name, as the user said it, for example "Umbreon VMAX" or "PSA 10 Charizard".' },
        max_price: { type: 'number', description: 'Highest price in pesos.' },
        min_price: { type: 'number', description: 'Lowest price in pesos.' },
        binder: { type: 'string', description: 'Only this binder. list_binders gives the names.' },
        graded: { type: 'boolean', description: 'Only graded cards (PSA, CGC, BGS and so on).' },
        sort: { type: 'string', enum: ['newest', 'price-asc', 'price-desc', 'title'], description: 'Result order. Default is newest first.' },
        limit: { type: 'integer', minimum: 1, maximum: 50, description: 'How many cards to return. Default 12.' },
      },
    },
    annotations: { readOnlyHint: true },
    execute: (input = {}, { signal } = {}) => {
      const q = new URLSearchParams();
      if (typeof input.query === 'string') q.set('q', input.query);
      for (const key of ['max_price', 'min_price', 'limit']) if (Number.isFinite(input[key])) q.set(key, String(input[key]));
      if (typeof input.binder === 'string') q.set('binder', input.binder);
      if (input.graded === true) q.set('graded', 'true');
      if (typeof input.sort === 'string' && input.sort !== 'newest') q.set('sort', input.sort);
      return get('/api/search?' + q, signal);
    },
  },
  {
    name: 'get_card',
    title: 'Card details',
    description: 'Get the full details of one card in Big Boys Vault: price, when the price was last updated, set, number, grade, copies held, binders and notes. Use the handle from search_cards.',
    inputSchema: { type: 'object', properties: { handle: { type: 'string', description: 'The card handle from search_cards.' } }, required: ['handle'] },
    annotations: { readOnlyHint: true },
    execute: (input = {}, { signal } = {}) => (typeof input.handle === 'string' && input.handle ? get('/api/card/' + encodeURIComponent(input.handle), signal) : { error: 'A card handle is needed. Use search_cards to find it.' }),
  },
  {
    name: 'list_binders',
    title: 'List the binders',
    description: 'List the binders in Big Boys Vault, with how many cards each holds and the total number of cards in the collection.',
    inputSchema: { type: 'object', properties: {} },
    annotations: { readOnlyHint: true },
    execute: (_input, { signal } = {}) => get('/api/binders', signal),
  },
];

for (const tool of tools) document.modelContext.registerTool(tool).catch((e) => console.debug('WebMCP tool not registered:', tool.name, e));
