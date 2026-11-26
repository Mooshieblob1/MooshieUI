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

/** Folded words for upper-body clothes, body/angles, and framing that pull beyond a portrait or fight PR. */
const UPPER_BODY_OR_CLOTHING_WORDS = new Set([
  // Upper-body garments & outerwear
  "shirt", "shirts", "tshirt", "tee", "blouse", "blouses", "top", "tops", "tank", "camisole",
  "sweater", "sweaters", "pullover", "cardigan", "hoodie", "hoodies", "jacket", "jackets",
  "coat", "coats", "overcoat", "trench", "blazer", "vest", "vests", "waistcoat", "suit", "suits",
  "tuxedo", "uniform", "uniforms", "serafuku", "dress", "dresses", "gown", "gowns", "sundress",
  "kimono", "yukata", "haori", "cheongsam", "qipao", "hanfu", "hanbok", "robe", "robes",
  "cloak", "cloaks", "cape", "capes", "capelet", "mantle", "poncho", "shawl", "apron",
  "overalls", "jumpsuit", "armor", "armour", "breastplate", "cuirass", "pauldron", "pauldrons",
  "corset", "bodice", "turtleneck", "necktie", "cravat", "ascot", "scarf", "scarves", "muffler",
  // Upper anatomy & framing/angles that pull out
  "shoulder", "shoulders", "collarbone", "clavicle", "nape",
  "full-length", "foreshortening", "perspective",
  // German
  "hemd", "hemden", "bluse", "blusen", "kleid", "kleider", "kleidchen", "jacke", "jacken",
  "mantel", "mäntel", "strickjacke", "schuluniform", "rüstung", "umhang", "schal", "krawatte",
  "schulter", "schultern", "schlüsselbein", "rücken", "flügel",
  // Spanish
  "camisa", "camisas", "blusa", "blusas", "vestido", "vestidos", "chaqueta", "chaquetas",
  "abrigo", "abrigos", "suéter", "sudadera", "uniforme", "uniformes", "armadura", "bufanda",
  "corbata", "hombro", "hombros", "clavícula", "espalda", "alas",
  // Portuguese
  "camiseta", "jaqueta", "jaquetas", "casaco", "casacos", "moletom", "cachecol", "gravata",
  "ombro", "ombros", "clavícula", "costas", "asas",
].map(fold));

const GERMAN_UPPER_HEADS = [
  "kleid", "kleider", "kleidchen", "hemd", "bluse", "jacke", "mantel", "pullover",
  "uniform", "rüstung", "umhang", "schal", "krawatte", "flügel",
].map(fold);

const UPPER_CLOTHING_PHRASES = [
  "sailor uniform", "sailor suit", "sailor dress", "sailor shirt", "sailor collar",
  "shirt collar", "high collar", "popped collar", "open collar", "fur collar", "lace collar",
  "collared shirt", "peter pan collar", "stand collar", "crop top", "tank top", "hood down",
  "wide shot", "long shot", "dynamic angle", "action pose", "three quarter view",
].map(fold);

const UPPER_CLOTHING_CJK = [
  "制服", "セーラー服", "シャツ", "ブラウス", "セーター", "パーカー", "ジャケット", "コート",
  "ドレス", "ワンピース", "着物", "浴衣", "和服", "鎧", "マント", "ケープ", "マフラー",
  "ネクタイ", "襟", "肩", "鎖骨", "背中", "翼",
  "水手服", "衬衫", "襯衫", "毛衣", "卫衣", "衛衣", "外套", "夹克", "夾克", "大衣", "风衣",
  "風衣", "连衣裙", "連衣裙", "长袍", "長袍", "旗袍", "汉服", "漢服", "铠甲", "鎧甲", "盔甲",
  "披风", "披風", "斗篷", "围巾", "圍巾", "领带", "領帶", "领结", "領結", "肩膀", "锁骨",
  "鎖骨", "背部", "翅膀",
];

function mentionsUpperClothingOrBody(text: string): boolean {
  if (UPPER_CLOTHING_CJK.some((term) => text.includes(term))) return true;
  const list = words(text).filter((w) => !/^[\d.]+$/.test(w));
  if (list.length === 0) return false;
  if (list.some((w) => UPPER_BODY_OR_CLOTHING_WORDS.has(w))) return true;
  if (list.some((w) => GERMAN_UPPER_HEADS.some((h) => w.length >= h.length + 3 && w.endsWith(h)))) return true;
  // Bare "wings" pulls upper/full body unless specifically head/hair/ear wings.
  if (list.includes("wings") && !list.some((w) => w === "head" || w === "hair" || w === "ear" || w === "ears")) {
    return true;
  }
  const joined = ` ${list.join(" ")} `;
  return UPPER_CLOTHING_PHRASES.some((p) => joined.includes(` ${p} `));
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

const EYE_AND_SKIN_COLOURS = new Set([
  ...COLOURS,
  "amber", "emerald", "crimson", "violet", "indigo", "magenta", "cyan", "aqua", "teal", "turquoise", "hazel",
].map(fold));

const FANTASY_SKIN_COLOURS = new Set([
  "blue", "red", "green", "purple", "violet", "indigo", "pink", "orange", "yellow",
  "grey", "gray", "black", "silver", "gold", "golden", "cyan", "aqua", "teal", "magenta",
  "colored", "multicolored", "two-tone", "split",
].map(fold));

const EYE_WORDS = new Set(["eye", "eyes", "iris", "irises", "augen", "ojos", "olhos"]);

const HETEROCHROMIA_PHRASES = [
  "heterochromia", "odd eyes", "mismatched eyes", "two-tone eyes", "split-color eyes",
  "verschiedenfarbige augen", "heterocromia",
].map(fold);

const HETEROCHROMIA_CJK = [
  "オッドアイ", "ヘテロクロミア", "左右色違い", "异色瞳", "異色瞳", "鸳鸯眼", "鴛鴦眼",
];

const SUBJECT_COUNT_TAGS = new Set(["1girl", "1boy", "1other", "girl", "boy", "other", "solo"]);

const PR_KEEP_WORDS = new Set([
  // Eyewear
  "glasses", "eyeglasses", "eyewear", "sunglasses", "monocle", "goggles", "eyepatch",
  "blindfold", "spectacles",
  "brille", "sonnenbrille", "monokel", "augenklappe",
  "gafas", "lentes", "anteojos", "monóculo",
  "óculos", "monóculo",
  // Earrings & piercings
  "earring", "earrings", "piercing", "piercings",
  "ohrring", "ohrringe", "arete", "aretes", "pendiente", "pendientes", "brinco", "brincos",
  // Hair ornaments & headwear
  "hairclip", "hairclips", "hairpin", "hairpins", "hairband", "hairbands", "headband", "headbands",
  "scrunchie", "scrunchies", "kanzashi", "barrette", "barrettes", "ribbon", "ribbons",
  "hat", "hats", "cap", "caps", "beret", "berets", "fedora", "beanie", "bonnet",
  "veil", "veils", "headscarf", "bandana", "kerchief", "crown", "crowns", "tiara", "tiaras",
  "diadem", "circlet", "headpiece", "headdress", "headphones", "headset", "earphones", "earmuffs",
  "hut", "mütze", "kappe", "krone", "schleier", "haarspange", "haarschmuck", "haarband",
  "haarschleife", "kopfhörer",
  "sombrero", "gorra", "boina", "corona", "velo", "auriculares",
  "chapéu", "boné", "coroa", "véu",
  // Neck accessories & masks
  "choker", "chokers", "necklace", "necklaces", "pendant", "pendants", "locket", "collar", "collars",
  "mask", "masks",
  // Facial marks, teeth, cosmetics & special eye anatomy
  "freckles", "mole", "moles", "scar", "scars", "scarred", "facepaint", "stitches",
  "bandaid", "bandaids", "makeup", "lipstick", "eyeshadow", "eyeliner", "mascara", "vitiligo",
  "fang", "fangs", "pupil", "pupils", "sclera", "eyelashes", "eyebrows",
  "sommersprossen", "narbe", "narben", "reißzahn", "reißzähne", "lippenstift",
  "pecas", "lunar", "lunares", "cicatriz", "cicatrices", "colmillo", "colmillos",
  "sardas", "pinta", "cicatrizes", "presas",
  // Non-human head features
  "kemonomimi", "horn", "horns", "antler", "antlers", "halo", "halos", "ahoge",
  "hörner", "geweih", "heiligenschein", "cuerno", "cuernos", "aureola", "chifres", "auréola",
  // Special eye patterns (always kept)
  "heterochromia",
].map(fold));

const PR_KEEP_PHRASES = [
  "hair ornament", "hair ornaments", "hair ribbon", "hair bow", "hair flower", "hair tie",
  "hair ties", "hair bell", "hair ring", "hair rings", "hair beads", "hair tube", "bobby pin",
  "beauty mark", "under eye", "dark circles", "bags under eyes", "whisker markings",
  "facial tattoo", "face tattoo", "cheek tattoo", "forehead tattoo", "neck tattoo",
  "sharp teeth", "shark teeth", "forked tongue", "split tongue", "long tongue",
  "pointy ears", "pointed ears", "elf ears", "animal ears", "cat ears", "dog ears",
  "fox ears", "wolf ears", "rabbit ears", "bunny ears", "cow ears", "horse ears",
  "sheep ears", "goat ears", "mouse ears", "dragon ears", "bat ears", "demon ears",
  "lion ears", "tiger ears", "bear ears", "deer ears", "fake animal ears", "floppy ears",
  "ear blush", "head wings", "hair wings", "ear wings", "antenna hair",
  "multicolored hair", "two tone hair", "split color hair", "gradient hair", "streaked hair",
  "colored inner hair", "mesh hair", "ombre hair", "peekaboo bangs",
  "slit pupils", "cat eyes", "constricted pupils", "heart shaped pupils", "star shaped pupils",
  "symbol shaped pupils", "cross shaped pupils", "flower shaped pupils", "diamond shaped pupils",
  "x shaped pupils", "vertical pupils", "ringed eyes", "empty eyes", "glowing eyes",
  "glowing eye", "sparkling eyes", "gradient eyes", "multicolored eyes", "rainbow eyes",
  "spiral eyes", "parche en el ojo", "tapa olho", "fones de ouvido", "dog tags",
].map(fold);

const PR_KEEP_CJK = [
  // Eyewear & masks
  "眼鏡", "メガネ", "めがね", "サングラス", "ゴーグル", "眼帯", "目隠し", "マスク",
  "眼镜", "墨镜", "护目镜", "護目鏡", "眼罩", "单片眼镜", "單片眼鏡", "口罩", "面具",
  // Earrings & neck/hair accessories
  "ピアス", "イヤリング", "チョーカー", "ネックレス", "ペンダント", "首輪",
  "帽子", "ベレー帽", "王冠", "ティアラ", "ベール", "ヴェール", "カチューシャ", "ヘアバンド",
  "ヘアピン", "髪飾り", "かんざし", "簪", "リボン", "ヘッドホン", "ヘッドフォン", "イヤホン",
  "ヘイロー", "光輪",
  "耳环", "耳環", "耳钉", "耳釘", "耳坠", "耳墜", "唇钉", "唇釘", "鼻环", "鼻環",
  "颈圈", "頸圈", "项圈", "項圈", "项链", "項鍊", "吊坠", "吊墜",
  "头饰", "頭飾", "发饰", "髮飾", "发卡", "髮卡", "发夹", "髮夾", "蝴蝶结", "蝴蝶結",
  "皇冠", "头纱", "頭紗", "耳机", "耳機", "贝雷帽", "貝雷帽", "光环", "光環",
  // Facial details, ears, horns, special hair, heterochromia
  "そばかす", "ほくろ", "黒子", "傷跡", "絆創膏", "八重歯", "エルフ耳", "尖った耳",
  "ケモミミ", "獣耳", "猫耳", "狐耳", "犬耳", "ウサギ耳", "うさ耳", "悪魔の角", "鬼の角",
  "一本角", "二本角", "アホ毛", "ツートンヘア", "インナーカラー", "メッシュ",
  "猫目", "ネコ目", "三白眼", "星目", "ハート目",
  "雀斑", "伤疤", "伤痕", "创可贴", "創可貼", "口红", "口紅", "犬齿", "犬齒", "虎牙", "尖牙",
  "精灵耳", "精靈耳", "尖耳", "兽耳", "獸耳", "貓耳", "兔耳", "恶魔角", "惡魔角",
  "尖角", "龙角", "龍角", "鹿角", "牛角", "羊角", "呆毛", "挑染", "渐变发", "漸變髮",
  "双色发", "雙色髮", "内层挑染", "內層挑染",
  ...HETEROCHROMIA_CJK,
];

const EYE_SHADE_MODIFIERS = new Set([
  "dark", "light", "pale", "dunkel", "hell", "oscuro", "claro", "escuro",
]);

function collectPromptEyeColors(prompt: string, out: Set<string>, hasCue: { value: boolean }): void {
  const f = fold(prompt);
  if (HETEROCHROMIA_CJK.some((t) => prompt.includes(t)) || HETEROCHROMIA_PHRASES.some((p) => f.includes(p))) {
    hasCue.value = true;
  }
  for (const item of splitTopLevel(prompt)) {
    const group = groupOf(item);
    if (group && splitTopLevel(group.inner).length > 1) {
      collectPromptEyeColors(group.inner, out, hasCue);
      continue;
    }
    const list = words(item).filter((w) => !/^[\d.]+$/.test(w));
    if (list.some((w) => EYE_WORDS.has(w))) {
      for (const w of list) {
        if (EYE_AND_SKIN_COLOURS.has(w) && !EYE_SHADE_MODIFIERS.has(w)) out.add(w);
      }
      if (list.includes("eye") && !list.includes("eyes") && list.some((w) => EYE_AND_SKIN_COLOURS.has(w))) {
        hasCue.value = true;
      }
    }
  }
}

function hasHeterochromiaCue(prompt: string): boolean {
  const colors = new Set<string>();
  const hasCue = { value: false };
  collectPromptEyeColors(prompt, colors, hasCue);
  return hasCue.value || colors.size >= 2;
}

function isPortraitPrEssential(text: string, heterochromia: boolean, keepSubjects: boolean): boolean {
  if (mentionsBelowNeck(text) || mentionsUpperClothingOrBody(text)) return false;
  const list = words(text).filter((w) => !/^[\d.]+$/.test(w));
  if (keepSubjects && list.length === 1 && SUBJECT_COUNT_TAGS.has(list[0])) return true;
  if (PR_KEEP_CJK.some((term) => text.includes(term))) return true;
  if (heterochromia && (text.includes("目") || text.includes("瞳") || text.includes("眼"))) return true;
  if (list.length === 0) return false;
  if (list.some((w) => PR_KEEP_WORDS.has(w))) return true;
  if (list.includes("skin") && list.some((w) => FANTASY_SKIN_COLOURS.has(w))) return true;
  if (heterochromia && list.some((w) => EYE_WORDS.has(w))) return true;
  if (list.includes("eye") && !list.includes("eyes") && list.some((w) => EYE_AND_SKIN_COLOURS.has(w))) return true;
  const joined = ` ${list.join(" ")} `;
  return PR_KEEP_PHRASES.some((p) => joined.includes(` ${p} `));
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

function filterTags(prompt: string, shouldDrop: (item: string) => boolean): PortraitFilterResult {
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
      if (shouldDrop(item)) dropped.push(item);
      else kept.push(item);
    }
    return kept;
  };
  return { kept: filterList(prompt).join(", "), dropped };
}

/**
 * Filter a comma-separated tag prompt. A weighted group that lists several
 * tags is filtered inside and kept with what is left.
 */
export function neckUpTags(prompt: string): PortraitFilterResult {
  return filterTags(prompt, mentionsBelowNeck);
}

/**
 * Aggressive portrait filter for Precise Reference: drops clothing, hair
 * length/color, single eye colors, character names, and poses so PR owns
 * identity and framing, keeping only accessories, heterochromia, unusual
 * eye/head anatomy, and facial markings that PR often misses.
 */
export function portraitPrTags(prompt: string): PortraitFilterResult {
  const heterochromia = hasHeterochromiaCue(prompt);
  return filterTags(prompt, (item) => !isPortraitPrEssential(item, heterochromia, true));
}

function filterProse(
  prompt: string,
  dropWholeClauseFirst: boolean,
  shouldDropClause: (clause: string) => boolean,
  shouldDropPart: (part: string) => boolean,
): PortraitFilterResult {
  const dropped: string[] = [];
  const clauses = prompt.split(/([,;、，；]\s*|(?<=[.!?])\s+|(?<=[。！？])\s*)/);
  const out: string[] = [];
  // Separators sit at odd indexes; each one belongs to the clause before it.
  for (let i = 0; i < clauses.length; i += 2) {
    const clause = clauses[i];
    const sep = clauses[i + 1] ?? "";
    if (!shouldDropClause(clause)) {
      const cleaned = out.length === 0 ? stripLeadingJoiner(clause) : clause;
      out.push(cleaned + sep);
      continue;
    }
    const kept = trimClause(clause, dropped, dropWholeClauseFirst, shouldDropPart);
    if (kept) {
      const cleaned = out.length === 0 ? stripLeadingJoiner(kept) : kept;
      out.push(cleaned + sep);
    } else if (/[.!?。！？]\s*$/.test(clause) && out.length > 0 && !/[.!?。！？]\s*$/.test(out[out.length - 1])) {
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

/**
 * Filter a sentence prompt. Clauses that mention something below the neck
 * lose the part that does: "wearing a sailor uniform with a pleated skirt and
 * knee socks" keeps "wearing a sailor uniform".
 */
export function neckUpProse(prompt: string): PortraitFilterResult {
  return filterProse(prompt, true, mentionsBelowNeck, mentionsBelowNeck);
}

/**
 * Precise Reference prose filter: keeps only segments describing
 * portrait-visible accessories, heterochromia, or distinctive facial/head
 * traits and drops body, pose, clothing, and plain appearance clauses.
 */
export function portraitPrProse(prompt: string): PortraitFilterResult {
  const heterochromia = hasHeterochromiaCue(prompt);
  const shouldDrop = (text: string) =>
    !isUserToken(text.trim()) && !isPortraitPrEssential(text, heterochromia, false);
  return filterProse(prompt, false, () => true, shouldDrop);
}

/** "with" and "and" in the Latin-script languages NovelAI V5 reads. */
const JOINERS = /\s+(with|and|plus|while|mit|und|sowie|con|y|com|e)\s+/i;
const LEADING_JOINER = /^(with|and|plus|while|mit|und|sowie|con|y|com|e)\s+/i;
const AND = /^(and|und|y|e)$/i;

function stripLeadingJoiner(text: string): string {
  return text.replace(LEADING_JOINER, "");
}

/**
 * One clause with unwanted parts cut at "with" and "and". When
 * `dropWholeOnHead` is true and the clause's own head is what goes ("She is
 * holding a katana and smiling"), the whole clause goes so no fragment stays.
 */
function trimClause(
  clause: string,
  dropped: string[],
  dropWholeOnHead: boolean,
  shouldDrop: (part: string) => boolean,
): string {
  const end = /[.!?。！？]$/.test(clause.trim()) ? clause.trim().slice(-1) : "";
  const body = end ? clause.trim().slice(0, -1) : clause.trim();
  if (!body) return "";
  const pieces = body.split(new RegExp(JOINERS.source, "i"));
  const parts: { sep: string; text: string }[] = [{ sep: "", text: pieces[0] }];
  for (let i = 1; i < pieces.length; i += 2) {
    const sep = pieces[i];
    const text = pieces[i + 1] ?? "";
    const prev = parts[parts.length - 1];
    const lastWord = fold(prev.text.trim().split(/\s+/).pop() ?? "");
    const firstWord = fold(text.trim().split(/\s+/)[0] ?? "");
    if (AND.test(sep) && EYE_AND_SKIN_COLOURS.has(lastWord) && EYE_AND_SKIN_COLOURS.has(firstWord)) {
      prev.text = `${prev.text} ${sep} ${text}`;
    } else {
      parts.push({ sep, text });
    }
  }
  if (dropWholeOnHead && shouldDrop(parts[0].text)) {
    dropped.push(body.trim());
    return "";
  }
  const kept: { sep: string; text: string }[] = [];
  let carry: string | null = null;
  let pushed = 0;
  for (const part of parts) {
    if (shouldDrop(part.text)) {
      const trimmed = part.text.trim();
      if (trimmed) {
        dropped.push(trimmed);
        pushed++;
      }
      // The first cut part's joining word goes to the next kept part.
      if (carry === null) carry = part.sep;
      continue;
    }
    kept.push({ sep: carry ?? part.sep, text: part.text });
    carry = null;
  }
  if (kept.length === 0) {
    // Replace the per-piece drops from this empty clause with the whole clause
    // so Left out chips read coherently.
    dropped.splice(dropped.length - pushed, pushed, body.trim());
    return "";
  }
  const text = kept.map((p, i) => (i === 0 ? p.text : `${p.sep} ${p.text}`)).join(" ").trim();
  return text + end;
}
