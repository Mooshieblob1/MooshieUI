/**
 * What a neck-up portrait leaves out of a character's prompt: everything
 * below the neck (body, legwear, shoes, poses, hands and what they hold) and
 * framings that fight a front-on close-up. Left in, those pull the model
 * into a wider shot or a pose the portrait cannot show.
 *
 * Knows the languages NovelAI V5 takes prompts in: English (and Danbooru
 * tags), Japanese, Chinese, German, Spanish and Portuguese.
 *
 * Leaf util with no imports, so it runs under plain Node for checking.
 */

/** Lowercase with accents taken off, so "pantalón" and "pantalon" match alike. Latin script only. */
function fold(text: string): string {
  return text.toLowerCase().normalize("NFD").replace(/\p{M}/gu, "");
}

/** Words that name something below the neck, matched as whole words. */
const BELOW_NECK_WORDS = new Set([
  // Body
  "arm", "arms", "armpit", "armpits", "elbow", "elbows", "forearm", "forearms", "wrist", "wrists",
  "hand", "hands", "palm", "palms", "finger", "fingers", "fingernails", "fist", "fists",
  "chest", "breast", "breasts", "cleavage", "underboob", "sideboob", "pectorals", "nipple", "nipples",
  "areola", "areolae", "torso", "stomach", "belly", "navel", "midriff", "abs", "waist", "hip", "hips",
  "butt", "ass", "crotch", "groin", "pubic", "pussy", "penis", "testicles", "genitals", "anus",
  "thigh", "thighs", "leg", "legs", "knee", "knees", "calf", "calves", "ankle", "ankles",
  "foot", "feet", "toe", "toes", "toenails", "sole", "soles", "barefoot", "tail", "tails",
  // Clothing below the neck
  "skirt", "skirts", "miniskirt", "microskirt", "pants", "trousers", "jeans", "shorts", "hakama",
  "panties", "underwear", "lingerie", "bra", "bikini", "swimsuit", "leotard", "bodysuit",
  "socks", "kneehighs", "thighhighs", "overkneehighs", "pantyhose", "stockings", "leggings",
  "legwear", "garter", "garters", "thighband", "anklet", "belt", "belts",
  "sock", "kneehigh", "thighhigh", "uwabaki",
  "shoe", "shoes", "boots", "sneakers", "loafers", "sandals", "slippers", "heels", "footwear", "geta", "zouri",
  "glove", "gloves", "mittens", "bracelet", "bracelets", "wristband", "wristbands", "sleeve", "sleeves",
  // Poses and actions
  "sitting", "standing", "kneeling", "squatting", "crouching", "lying", "reclining", "walking", "running",
  "jumping", "leaning", "seiza", "wariza", "contrapposto", "pose", "posing", "holding", "carrying",
  "pointing", "waving", "salute", "reaching",
  // Things carried
  "weapon", "weapons", "sword", "swords", "katana", "gun", "guns", "rifle", "pistol", "handgun",
  "spear", "polearm", "shield", "axe", "dagger", "knife", "staff", "wand", "scythe",
  "bag", "backpack", "handbag", "briefcase", "umbrella", "book", "phone", "cellphone", "smartphone",
  // German
  "arme", "armen", "hände", "händen", "fingern", "brust", "brüste", "busen", "dekolleté", "bauch",
  "bauchnabel", "nabel", "taille", "hüfte", "hüften", "hintern", "po", "oberschenkel", "schenkel", "bein",
  "beine", "beinen", "knie", "knien", "fuß", "füße", "füßen", "fuss", "füsse", "zehen", "barfuß", "barfuss",
  "schwanz", "röcke", "minirock", "hose", "hosen", "strümpfe", "strumpfhose", "socken", "schuhe", "stiefel",
  "sandalen", "handschuhe", "gürtel", "badeanzug", "unterwäsche", "ärmel", "armband", "sitzend", "stehend",
  "kniend", "liegend", "hockend", "laufend", "rennend", "springend", "haltend", "schwert", "waffe", "waffen",
  "gewehr", "pistole", "speer", "schild", "tasche", "rucksack", "regenschirm", "buch", "handy",
  "ganzkörper", "ganzkörperansicht", "oberkörper", "profil", "seitenansicht", "rückansicht",
  // Spanish
  "brazo", "brazos", "mano", "manos", "dedo", "dedos", "pecho", "pechos", "senos", "escote", "vientre",
  "barriga", "ombligo", "cintura", "cadera", "caderas", "trasero", "nalgas", "muslo", "muslos", "pierna",
  "piernas", "rodilla", "rodillas", "pie", "pies", "descalza", "descalzo", "falda", "faldas", "minifalda",
  "pantalón", "pantalones", "vaqueros", "bragas", "sujetador", "bañador", "calcetines", "medias",
  "pantimedias", "zapatos", "botas", "zapatillas", "sandalias", "tacones", "guantes", "cinturón", "pulsera",
  "pulseras", "sentada", "sentado", "arrodillada", "arrodillado", "agachada", "agachado", "acostada",
  "acostado", "tumbada", "tumbado", "caminando", "corriendo", "saltando", "posando", "sosteniendo",
  "sujetando", "espada", "arma", "armas", "bastón", "lanza", "escudo", "bolso", "mochila", "paraguas",
  "libro", "teléfono", "móvil",
  // Portuguese
  "braço", "braços", "mão", "mãos", "peito", "peitos", "seios", "decote", "umbigo", "quadril", "quadris",
  "bumbum", "nádegas", "coxa", "coxas", "perna", "pernas", "joelho", "joelhos", "pé", "pés", "descalça",
  "descalço", "cauda", "rabo", "saia", "saias", "minissaia", "calça", "calças", "calções", "calcinha",
  "sutiã", "biquíni", "maiô", "meias", "sapatos", "tênis", "ténis", "sandálias", "saltos", "luvas", "cinto",
  "pulseira", "pulseiras", "ajoelhada", "ajoelhado", "deitada", "deitado", "andando", "caminhando",
  "correndo", "pulando", "segurando", "cajado", "lança", "bolsa", "livro", "celular", "telemóvel",
].map(fold));

/**
 * German builds clothes and body words as compounds with the head last
 * (Faltenrock, Lederstiefel, Kniestrümpfe), so a word ending in one of these
 * counts too, when at least three letters come before it.
 */
const GERMAN_HEADS = [
  "rock", "röcke", "röcken", "hose", "hosen", "strümpfe", "strümpfen", "strumpf", "socken", "schuhe", "schuhen",
  "schuh", "stiefel", "stiefeln", "sandalen", "gürtel", "gürteln", "badeanzug", "unterwäsche", "ärmel", "ärmeln",
  "beine", "beinen", "schenkel", "schenkeln", "tasche", "schwert", "waffe",
].map(fold);

/**
 * Japanese and Chinese terms, matched anywhere in the text since neither
 * language spaces its words. Single characters with common unrelated uses
 * (手 in 歌手, 足 in 満足, 剣 in 真剣) are only matched in longer forms.
 */
const CJK_TERMS = [
  // Japanese: body
  "腕", "胸", "おっぱい", "谷間", "お腹", "おなか", "へそ", "臍", "お尻", "太もも", "太腿", "ふともも", "脚", "膝",
  "素足", "裸足", "足元", "両足", "両手", "片手", "手を", "手に", "手で", "指先", "尻尾", "しっぽ",
  // Japanese: clothing below the neck
  "スカート", "ズボン", "パンツ", "ジーンズ", "靴下", "ソックス", "ニーソ", "ニーハイ", "タイツ", "ストッキング",
  "レギンス", "ブーツ", "スニーカー", "ローファー", "サンダル", "ハイヒール", "上履き", "上靴", "靴", "手袋",
  "グローブ", "水着", "ビキニ", "下着", "ブラジャー", "レオタード", "ベルト", "ガーター", "ブレスレット", "腕輪",
  // Japanese: poses, framings, things carried
  "座っ", "座り", "立っ", "立ち姿", "跪", "しゃがん", "寝そべ", "横たわ", "歩い", "走っ", "ジャンプ", "ポーズ",
  "持っ", "抱え", "握っ", "ピースサイン", "Vサイン", "全身", "上半身", "下半身", "横顔", "後ろ姿", "振り返",
  "武器", "銃", "日本刀", "大剣", "剣を", "刀を", "杖を", "槍", "盾を", "鞄", "かばん", "バッグ", "リュック",
  "傘を", "スマホ",
  // Chinese (simplified and traditional): body
  "手臂", "胳膊", "双手", "雙手", "手指", "胸部", "乳沟", "乳溝", "肚子", "腹部", "肚脐", "肚臍", "臀", "屁股",
  "大腿", "腿", "膝盖", "膝蓋", "赤脚", "赤腳", "光脚", "光腳", "赤足", "脚", "腳", "尾巴",
  // Chinese: clothing below the neck
  "裙子", "短裙", "百褶裙", "长裙", "長裙", "迷你裙", "半身裙", "裤", "褲", "袜", "襪", "鞋", "靴", "高跟",
  "手套", "泳装", "泳裝", "比基尼", "内衣", "內衣", "胸罩", "腰带", "腰帶", "皮带", "皮帶", "手镯", "手鐲",
  // Chinese: poses, framings, things carried
  "坐着", "坐著", "坐在", "站着", "站著", "站立", "跪", "蹲", "躺", "走路", "奔跑", "跳跃", "跳躍", "姿势",
  "姿勢", "手持", "拿着", "拿著", "握着", "握著", "举着", "舉著", "剪刀手", "侧脸", "側臉", "背影", "剑", "劍",
  "枪", "槍", "法杖", "长矛", "長矛", "背包", "书包", "書包", "手提包", "雨伞", "雨傘", "手机", "手機",
];

/** Several-word phrases: framings, views and gestures a front-on close-up cannot have. */
const BELOW_NECK_PHRASES = [
  "full body", "upper body", "lower body", "cowboy shot", "wide shot", "out of frame",
  "from side", "from behind", "from below", "from above", "dutch angle", "multiple views",
  "looking back", "looking away", "looking to the side", "on back", "on side", "all fours", "bent over",
  "peace sign", "thumbs up", "double v", "mary janes",
  // German
  "von hinten", "von der seite", "von unten", "von oben",
  // Spanish
  "de pie", "ropa interior", "traje de baño", "cuerpo entero", "cuerpo completo", "medio cuerpo",
  "de perfil", "de espaldas", "desde atrás", "desde abajo",
  // Portuguese
  "em pé", "de pé", "roupa íntima", "fato de banho", "meia calça", "salto alto", "corpo inteiro",
  "de costas", "por trás", "guarda chuva",
].map(fold);

/** Colours, so "a black and white skirt" is read as one thing, not cut after "black". */
const COLOURS = new Set([
  "black", "white", "red", "blue", "green", "yellow", "purple", "pink", "orange", "brown", "grey", "gray",
  "silver", "gold", "golden", "navy", "beige", "cream", "dark", "light", "pale",
  "schwarz", "weiß", "weiss", "rot", "blau", "grün", "gelb", "lila", "rosa", "braun", "grau", "silbern", "dunkel", "hell",
  "negro", "negra", "blanco", "blanca", "rojo", "roja", "azul", "verde", "amarillo", "amarilla", "morado",
  "morada", "naranja", "marrón", "gris", "plateado", "plateada", "dorado", "dorada", "oscuro", "claro",
  "preto", "preta", "branco", "branca", "vermelho", "vermelha", "amarelo", "amarela", "roxo", "roxa",
  "laranja", "marrom", "castanho", "cinza", "prateada", "dourado", "dourada", "escuro",
].map(fold));

/** Whole tags that only mean something below the neck on their own. */
const BELOW_NECK_TAGS = new Set(["v", "profile"]);

/** Folded words of a prompt piece, with underscores and hyphens read as spaces. */
function words(text: string): string[] {
  return fold(text)
    .replace(/[_-]/g, " ")
    .split(/[^\p{L}\p{N}']+/u)
    .filter(Boolean);
}

function germanCompound(word: string): boolean {
  return GERMAN_HEADS.some((head) => word.length >= head.length + 3 && word.endsWith(head));
}

/** Does this piece of a prompt name something a neck-up, front-on portrait cannot show? */
export function mentionsBelowNeck(text: string): boolean {
  if (CJK_TERMS.some((term) => text.includes(term))) return true;
  const list = words(text).filter((w) => !/^[\d.]+$/.test(w));
  if (list.length === 0) return false;
  if (list.some((w) => BELOW_NECK_WORDS.has(w) || germanCompound(w))) return true;
  if (BELOW_NECK_TAGS.has(list.join(" "))) return true;
  const joined = ` ${list.join(" ")} `;
  return BELOW_NECK_PHRASES.some((p) => joined.includes(` ${p} `));
}

/**
 * Split at commas (Japanese and Chinese ones too) and newlines that are not
 * inside brackets or a NovelAI `1.2::...::` emphasis block, so a weighted
 * group stays whole.
 */
export function splitTopLevel(text: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let inBlock = false;
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === "\\") {
      i++;
      continue;
    }
    if (ch === "(" || ch === "[" || ch === "{") depth++;
    else if ((ch === ")" || ch === "]" || ch === "}") && depth > 0) depth--;
    else if (ch === ":" && text[i + 1] === ":") {
      // `1.2::` opens a block; the next `::` closes it.
      inBlock = inBlock ? false : /-?[\d.]+\s*$/.test(text.slice(start, i));
      i++;
    } else if ((ch === "," || ch === "\n" || ch === "、" || ch === "，") && depth === 0 && !inBlock) {
      out.push(text.slice(start, i));
      start = i + 1;
    }
  }
  out.push(text.slice(start));
  return out.map((s) => s.trim()).filter(Boolean);
}

/** The wrapper around a weighted group, or null when the item is a plain tag. */
function groupOf(item: string): { open: string; inner: string; close: string } | null {
  const nai = /^(-?[\d.]+\s*::)([\s\S]*?)(::)$/.exec(item);
  if (nai) return { open: nai[1], inner: nai[2], close: nai[3] };
  const paren = /^(\(+)([\s\S]*?)((?::\s*-?[\d.]+)?\)+)$/.exec(item);
  if (paren) return { open: paren[1], inner: paren[2], close: paren[3] };
  const other = /^([[{]+)([\s\S]*?)([\]}]+)$/.exec(item);
  if (other && !other[2].includes("|")) return { open: other[1], inner: other[2], close: other[3] };
  return null;
}

/** Artist tags and inline chunks are the user's picks, never filtered. */
function isUserToken(item: string): boolean {
  return /^@/.test(item) || /^artist:/i.test(item);
}

export interface PortraitFilterResult {
  /** The prompt with what a portrait cannot show taken out. */
  kept: string;
  /** What was taken out, in prompt order, so the user can put any of it back. */
  dropped: string[];
}

/**
 * Filter a comma-separated tag prompt. A weighted group that lists several
 * tags is filtered inside and kept with what is left.
 */
export function neckUpTags(prompt: string): PortraitFilterResult {
  const dropped: string[] = [];
  const filterList = (text: string): string[] => {
    const kept: string[] = [];
    for (const item of splitTopLevel(text)) {
      if (isUserToken(item)) {
        kept.push(item);
        continue;
      }
      const group = groupOf(item);
      if (group && splitTopLevel(group.inner).length > 1) {
        const inner = filterList(group.inner);
        if (inner.length > 0) kept.push(`${group.open}${inner.join(", ")}${group.close}`);
        continue;
      }
      if (mentionsBelowNeck(item)) dropped.push(item);
      else kept.push(item);
    }
    return kept;
  };
  return { kept: filterList(prompt).join(", "), dropped };
}

/**
 * Filter a sentence prompt. Clauses that mention something below the neck
 * lose the part that does: "wearing a sailor uniform with a pleated skirt and
 * knee socks" keeps "wearing a sailor uniform".
 */
export function neckUpProse(prompt: string): PortraitFilterResult {
  const dropped: string[] = [];
  const clauses = prompt.split(/([,;、，；]\s*|(?<=[.!?])\s+|(?<=[。！？])\s*)/);
  const out: string[] = [];
  // Separators sit at odd indexes; each one belongs to the clause before it.
  for (let i = 0; i < clauses.length; i += 2) {
    const clause = clauses[i];
    const sep = clauses[i + 1] ?? "";
    if (!mentionsBelowNeck(clause)) {
      out.push(clause + sep);
      continue;
    }
    const kept = trimClause(clause, dropped);
    if (kept) out.push(kept + sep);
    else if (/[.!?。！？]\s*$/.test(clause) && out.length > 0 && !/[.!?。！？]\s*$/.test(out[out.length - 1])) {
      // A sentence that ends here keeps its full stop on what came before.
      const cjk = /[。！？]\s*$/.test(clause);
      out[out.length - 1] = out[out.length - 1].replace(/[,;、，；]\s*$/, "") + (cjk ? "。" : ". ");
    }
  }
  const kept = out
    .join("")
    .replace(/\s+([,.;])/g, "$1")
    .replace(/[,;、，；]\s*$/, "")
    .trim();
  return { kept, dropped };
}

/** "with" and "and" in the Latin-script languages NovelAI V5 reads. */
const JOINERS = /\s+(with|and|plus|while|mit|und|sowie|con|y|com|e)\s+/i;
const AND = /^(and|und|y|e)$/i;

/**
 * One clause with its below-the-neck parts cut at "with" and "and". When the
 * clause's own head is what goes ("She is holding a katana and smiling"), the
 * whole clause goes, since what is left would not read as a sentence. Japanese
 * and Chinese clauses go whole.
 */
function trimClause(clause: string, dropped: string[]): string {
  const end = /[.!?。！？]$/.test(clause.trim()) ? clause.trim().slice(-1) : "";
  const body = end ? clause.trim().slice(0, -1) : clause.trim();
  const pieces = body.split(new RegExp(JOINERS.source, "i"));
  const parts: { sep: string; text: string }[] = [{ sep: "", text: pieces[0] }];
  for (let i = 1; i < pieces.length; i += 2) {
    const sep = pieces[i];
    const text = pieces[i + 1] ?? "";
    const prev = parts[parts.length - 1];
    const lastWord = fold(prev.text.trim().split(/\s+/).pop() ?? "");
    const firstWord = fold(text.trim().split(/\s+/)[0] ?? "");
    if (AND.test(sep) && COLOURS.has(lastWord) && COLOURS.has(firstWord)) {
      prev.text = `${prev.text} ${sep} ${text}`;
    } else {
      parts.push({ sep, text });
    }
  }
  if (mentionsBelowNeck(parts[0].text)) {
    dropped.push(body.trim());
    return "";
  }
  const kept: { sep: string; text: string }[] = [];
  let carry: string | null = null;
  for (const part of parts) {
    if (mentionsBelowNeck(part.text)) {
      dropped.push(part.text.trim());
      // The first cut part's joining word goes to the next kept part.
      if (carry === null) carry = part.sep;
      continue;
    }
    kept.push({ sep: carry ?? part.sep, text: part.text });
    carry = null;
  }
  if (kept.length === 0) return "";
  const text = kept.map((p, i) => (i === 0 ? p.text : `${p.sep} ${p.text}`)).join(" ").trim();
  return text + end;
}
