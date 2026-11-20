// Vault Arcade's Pokémon: the first 151, with today's types (Fairy and Steel included), and what each type hits hard.
// Pictures come from PokeAPI's public sprite library by number: `art(n)` is the large artwork (silhouettes, Type
// Challenge), `sprite(n)` the small one (Memory Match, light enough for 64 cards on a phone).
window.ArcadeDex = (() => {
  const RAW = `Bulbasaur:grass/poison|Ivysaur:grass/poison|Venusaur:grass/poison|Charmander:fire|Charmeleon:fire|Charizard:fire/flying|Squirtle:water|Wartortle:water|Blastoise:water|Caterpie:bug|Metapod:bug|Butterfree:bug/flying|Weedle:bug/poison|Kakuna:bug/poison|Beedrill:bug/poison|Pidgey:normal/flying|Pidgeotto:normal/flying|Pidgeot:normal/flying|Rattata:normal|Raticate:normal|Spearow:normal/flying|Fearow:normal/flying|Ekans:poison|Arbok:poison|Pikachu:electric|Raichu:electric|Sandshrew:ground|Sandslash:ground|Nidoran♀:poison|Nidorina:poison|Nidoqueen:poison/ground|Nidoran♂:poison|Nidorino:poison|Nidoking:poison/ground|Clefairy:fairy|Clefable:fairy|Vulpix:fire|Ninetales:fire|Jigglypuff:normal/fairy|Wigglytuff:normal/fairy|Zubat:poison/flying|Golbat:poison/flying|Oddish:grass/poison|Gloom:grass/poison|Vileplume:grass/poison|Paras:bug/grass|Parasect:bug/grass|Venonat:bug/poison|Venomoth:bug/poison|Diglett:ground|Dugtrio:ground|Meowth:normal|Persian:normal|Psyduck:water|Golduck:water|Mankey:fighting|Primeape:fighting|Growlithe:fire|Arcanine:fire|Poliwag:water|Poliwhirl:water|Poliwrath:water/fighting|Abra:psychic|Kadabra:psychic|Alakazam:psychic|Machop:fighting|Machoke:fighting|Machamp:fighting|Bellsprout:grass/poison|Weepinbell:grass/poison|Victreebel:grass/poison|Tentacool:water/poison|Tentacruel:water/poison|Geodude:rock/ground|Graveler:rock/ground|Golem:rock/ground|Ponyta:fire|Rapidash:fire|Slowpoke:water/psychic|Slowbro:water/psychic|Magnemite:electric/steel|Magneton:electric/steel|Farfetch'd:normal/flying|Doduo:normal/flying|Dodrio:normal/flying|Seel:water|Dewgong:water/ice|Grimer:poison|Muk:poison|Shellder:water|Cloyster:water/ice|Gastly:ghost/poison|Haunter:ghost/poison|Gengar:ghost/poison|Onix:rock/ground|Drowzee:psychic|Hypno:psychic|Krabby:water|Kingler:water|Voltorb:electric|Electrode:electric|Exeggcute:grass/psychic|Exeggutor:grass/psychic|Cubone:ground|Marowak:ground|Hitmonlee:fighting|Hitmonchan:fighting|Lickitung:normal|Koffing:poison|Weezing:poison|Rhyhorn:ground/rock|Rhydon:ground/rock|Chansey:normal|Tangela:grass|Kangaskhan:normal|Horsea:water|Seadra:water|Goldeen:water|Seaking:water|Staryu:water|Starmie:water/psychic|Mr. Mime:psychic/fairy|Scyther:bug/flying|Jynx:ice/psychic|Electabuzz:electric|Magmar:fire|Pinsir:bug|Tauros:normal|Magikarp:water|Gyarados:water/flying|Lapras:water/ice|Ditto:normal|Eevee:normal|Vaporeon:water|Jolteon:electric|Flareon:fire|Porygon:normal|Omanyte:rock/water|Omastar:rock/water|Kabuto:rock/water|Kabutops:rock/water|Aerodactyl:rock/flying|Snorlax:normal|Articuno:ice/flying|Zapdos:electric/flying|Moltres:fire/flying|Dratini:dragon|Dragonair:dragon|Dragonite:dragon/flying|Mewtwo:psychic|Mew:psychic`;
  const MONS = RAW.split('|').map((s, i) => { const [name, t] = s.split(':'); return { id: i + 1, name, types: t.split('/') }; });
  // The ones most people know: Who's That Pokémon? starts with these.
  const EASY = [1, 4, 6, 7, 9, 25, 26, 35, 37, 39, 52, 54, 58, 59, 63, 66, 74, 92, 94, 95, 104, 113, 120, 129, 130, 131, 133, 134, 135, 136, 143, 144, 145, 146, 147, 149, 150, 151, 3, 12];
  const TYPES = ['normal', 'fire', 'water', 'electric', 'grass', 'ice', 'fighting', 'poison', 'ground', 'flying', 'psychic', 'bug', 'rock', 'ghost', 'dragon', 'dark', 'steel', 'fairy'];
  // An attacking type → the types it is super effective against.
  const STRONG = {
    normal: [], fire: ['grass', 'ice', 'bug', 'steel'], water: ['fire', 'ground', 'rock'], electric: ['water', 'flying'],
    grass: ['water', 'ground', 'rock'], ice: ['grass', 'ground', 'flying', 'dragon'], fighting: ['normal', 'ice', 'rock', 'dark', 'steel'],
    poison: ['grass', 'fairy'], ground: ['fire', 'electric', 'poison', 'rock', 'steel'], flying: ['grass', 'fighting', 'bug'],
    psychic: ['fighting', 'poison'], bug: ['grass', 'psychic', 'dark'], rock: ['fire', 'ice', 'flying', 'bug'], ghost: ['psychic', 'ghost'],
    dragon: ['dragon'], dark: ['psychic', 'ghost'], steel: ['ice', 'rock', 'fairy'], fairy: ['fighting', 'dragon', 'dark'],
  };
  const COLOR = {
    normal: '#a8a77a', fire: '#ee8130', water: '#6390f0', electric: '#f7d02c', grass: '#7ac74c', ice: '#96d9d6', fighting: '#c22e28',
    poison: '#a33ea1', ground: '#e2bf65', flying: '#a98ff3', psychic: '#f95587', bug: '#a6b91a', rock: '#b6a136', ghost: '#735797',
    dragon: '#6f35fc', dark: '#705746', steel: '#b7b7ce', fairy: '#d685ad',
  };
  const art = (n) => `https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/other/official-artwork/${n}.png`;
  const sprite = (n) => `https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/${n}.png`;
  const cap = (t) => t.charAt(0).toUpperCase() + t.slice(1);
  // Which attacking types hit this defending type hard.
  const beats = (t) => TYPES.filter((a) => STRONG[a].includes(t));
  return { MONS, EASY, TYPES, STRONG, COLOR, art, sprite, cap, beats, byId: (n) => MONS[n - 1] };
})();
