/**
 * Generates scripts/i18n-artist-gallery-all.json from en.ts keys + per-locale maps.
 * Run: node scripts/generate-artist-gallery-i18n.mjs
 * Then: node scripts/apply-i18n-gaps.mjs i18n-artist-gallery-all.json
 */
import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(__dirname, "..");

function parseLocaleKeys(file, prefix) {
  const text = fs.readFileSync(path.join(root, file), "utf8");
  const map = {};
  const re = new RegExp(`"(${prefix.replace(/\./g, "\\.")}[^"]+)":\\s*"((?:[^"\\\\]|\\\\.)*)"`, "g");
  let m;
  while ((m = re.exec(text)) !== null) {
    map[m[1]] = m[2].replace(/\\"/g, '"').replace(/\\n/g, "\n");
  }
  return map;
}

const en = parseLocaleKeys("src/lib/locales/en.ts", "artist_gallery.");
const keys = Object.keys(en).sort();

/** @type {Record<string, Record<string, string>>} */
const T = {
  ja: {
    "artist_gallery.title": "アーティストギャラリー",
    "artist_gallery.subtitle": "{count} アーティスト · Anima プレビュー · リリース {release} ·",
    "artist_gallery.gen_params_btn": "ℹ 生成パラメータ",
    "artist_gallery.manifest_error": "読み込み失敗: {error}",
    "artist_gallery.loading_manifest": "マニフェストを読み込み中…",
    "artist_gallery.search_placeholder": "アーティストタグを検索…",
    "artist_gallery.sort_label": "並べ替え:",
    "artist_gallery.sort_post_count": "投稿数",
    "artist_gallery.sort_name": "名前",
    "artist_gallery.sort_trending": "トレンド",
    "artist_gallery.sort_trending_title": "隠れた名作を表示 — まだ過剰に露出していない個性的なスタイルのアーティスト",
    "artist_gallery.sort_rotate": "↻ 再シャッフル",
    "artist_gallery.sort_rotate_title": "トレンド順を再シャッフルして新しい隠れた名作を発見",
    "artist_gallery.sort_desc": "↓ 降順",
    "artist_gallery.sort_desc_title": "降順",
    "artist_gallery.sort_asc": "↑ 昇順",
    "artist_gallery.sort_asc_title": "昇順",
    "artist_gallery.per_page_label": "1ページあたり:",
    "artist_gallery.size_label": "サイズ:",
    "artist_gallery.favourites_btn": "お気に入り",
    "artist_gallery.favourites_title": "お気に入りフィルターの切り替え",
    "artist_gallery.manage_btn": "⚙ 管理",
    "artist_gallery.manage_title": "カテゴリの管理、お気に入りのインポート/エクスポート",
    "artist_gallery.category_label": "カテゴリ:",
    "artist_gallery.category_all": "すべて ({count})",
    "artist_gallery.category_uncat": "未分類 ({count})",
    "artist_gallery.load_error": "アーティストの読み込みに失敗: {error}",
    "artist_gallery.loading_artists": "アーティストを読み込み中…",
    "artist_gallery.searching": "検索中…",
    "artist_gallery.search_result_one": "「{query}」の結果 1 件",
    "artist_gallery.search_results": "「{query}」の結果 {count} 件",
    "artist_gallery.hint_copy": "カードを右クリックするかホバーしてタグをコピー。",
    "artist_gallery.prev": "← 前へ",
    "artist_gallery.next": "次へ →",
    "artist_gallery.random": "⚄ ランダム",
    "artist_gallery.random_title": "ランダムなページへジャンプ",
    "artist_gallery.page_of": "{total} ページ中 {page} ページ",
    "artist_gallery.artist_count": "{count} アーティスト",
    "artist_gallery.page_placeholder": "ページ #",
    "artist_gallery.go_to_page": "ページへ移動",
    "artist_gallery.no_preview": "プレビューなし",
    "artist_gallery.copy_btn": "コピー",
    "artist_gallery.copy_tag_aria": "タグをコピー",
    "artist_gallery.copied": "コピーしました！",
    "artist_gallery.assign_category_aria": "カテゴリを割り当て",
    "artist_gallery.change_category_aria": "カテゴリ: {name}。クリックして変更。",
    "artist_gallery.add_fav_aria": "お気に入りに追加",
    "artist_gallery.remove_fav_aria": "お気に入りから削除",
    "artist_gallery.add_fav_title": "お気に入りに追加",
    "artist_gallery.remove_fav_title": "お気に入りから削除 · 右クリックで分類",
    "artist_gallery.close_cat_picker": "カテゴリ選択を閉じる",
    "artist_gallery.cat_uncategorised": "未分類",
    "artist_gallery.cat_new": "＋ 新しいカテゴリ…",
    "artist_gallery.card_title": "{tag} · 右クリックでタグをコピー",
    "artist_gallery.gen_params.title": "プレビュー生成パラメータ",
    "artist_gallery.gen_params.model_stack": "モデルスタック",
    "artist_gallery.gen_params.sampler_section": "サンプラー",
    "artist_gallery.gen_params.positive": "ポジティブプロンプト",
    "artist_gallery.gen_params.negative": "ネガティブプロンプト",
    "artist_gallery.gen_params.unet": "UNet",
    "artist_gallery.gen_params.text_encoder": "テキストエンコーダー",
    "artist_gallery.gen_params.vae": "VAE",
    "artist_gallery.gen_params.sampler": "サンプラー",
    "artist_gallery.gen_params.scheduler": "スケジューラー",
    "artist_gallery.gen_params.steps": "ステップ数",
    "artist_gallery.gen_params.cfg_scale": "CFGスケール",
    "artist_gallery.gen_params.seed": "シード",
    "artist_gallery.gen_params.resolution": "解像度",
    "artist_gallery.gen_params.output": "出力",
    "artist_gallery.lightbox.aria": "アーティストプレビュー: {tag}",
    "artist_gallery.lightbox.close_aria": "閉じる",
    "artist_gallery.lightbox.prev_aria": "前のアーティスト",
    "artist_gallery.lightbox.next_aria": "次のアーティスト",
    "artist_gallery.lightbox.no_preview": "プレビューは利用できません",
    "artist_gallery.lightbox.posts": "{count} 投稿",
    "artist_gallery.lightbox.aliases": "· 別名: {list}",
    "artist_gallery.lightbox.copy_tag": "タグをコピー",
    "artist_gallery.lightbox.insert": "プロンプトに挿入",
    "artist_gallery.lightbox.close": "閉じる",
    "artist_gallery.fav_manager.aria": "お気に入りカテゴリを管理",
    "artist_gallery.fav_manager.close_aria": "閉じる",
    "artist_gallery.fav_manager.title": "お気に入り · カテゴリとバックアップ",
    "artist_gallery.fav_manager.categories": "カテゴリ",
    "artist_gallery.fav_manager.name_label": "名前",
    "artist_gallery.fav_manager.colour_label": "色",
    "artist_gallery.fav_manager.pick_colour": "色 {colour} を選択",
    "artist_gallery.fav_manager.custom_colour": "カスタム色",
    "artist_gallery.fav_manager.add_btn": "追加",
    "artist_gallery.fav_manager.name_placeholder": "例: ポートレート",
    "artist_gallery.fav_manager.empty": "カテゴリはまだありません。上で作成してお気に入りを整理してください。",
    "artist_gallery.fav_manager.fav_one": "お気に入り 1",
    "artist_gallery.fav_manager.fav_count": "お気に入り {count}",
    "artist_gallery.fav_manager.edit_btn": "編集",
    "artist_gallery.fav_manager.delete_btn": "削除",
    "artist_gallery.fav_manager.save_btn": "保存",
    "artist_gallery.fav_manager.cancel_btn": "キャンセル",
    "artist_gallery.fav_manager.cat_colour": "カテゴリの色",
    "artist_gallery.fav_manager.delete_confirm": "カテゴリ「{name}」を削除しますか？",
    "artist_gallery.fav_manager.delete_confirm_used": "カテゴリ「{name}」を削除しますか？ {count} 件のお気に入りが未分類になります。",
    "artist_gallery.fav_manager.backup": "バックアップ",
    "artist_gallery.fav_manager.backup_desc": "お気に入りとカテゴリを .json ファイルにエクスポートするか、保存済みファイルをインポートします。",
    "artist_gallery.fav_manager.export_btn": "⬇ お気に入りをエクスポート…",
    "artist_gallery.fav_manager.import_mode": "インポートモード:",
    "artist_gallery.fav_manager.merge": "マージ",
    "artist_gallery.fav_manager.replace": "置換",
    "artist_gallery.fav_manager.import_btn": "⬆ お気に入りをインポート…",
    "artist_gallery.fav_manager.import_result_cat_one": "インポート完了 · {added} 追加、{updated} 更新、{categories} 件の新カテゴリ。",
    "artist_gallery.fav_manager.import_result_cats": "インポート完了 · {added} 追加、{updated} 更新、{categories} 件の新カテゴリ。",
    "artist_gallery.fav_manager.import_error": "エラー: {error}",
  },
  fr: {
    "artist_gallery.title": "Galerie d'artistes",
    "artist_gallery.subtitle": "{count} artistes · Aperçu Anima · version {release} ·",
    "artist_gallery.gen_params_btn": "ℹ param. gén.",
    "artist_gallery.manifest_error": "échec du chargement : {error}",
    "artist_gallery.loading_manifest": "chargement du manifeste…",
    "artist_gallery.search_placeholder": "Rechercher un tag d'artiste…",
    "artist_gallery.sort_label": "Tri :",
    "artist_gallery.sort_post_count": "Nombre de posts",
    "artist_gallery.sort_name": "Nom",
    "artist_gallery.sort_trending": "Tendances",
    "artist_gallery.sort_trending_title": "Met en avant des pépites : artistes au style distinctif pas encore sur-exposés",
    "artist_gallery.sort_rotate": "↻ Mélanger",
    "artist_gallery.sort_rotate_title": "Remélanger le classement tendances pour découvrir de nouvelles pépites",
    "artist_gallery.sort_desc": "↓ Desc",
    "artist_gallery.sort_desc_title": "Décroissant",
    "artist_gallery.sort_asc": "↑ Asc",
    "artist_gallery.sort_asc_title": "Croissant",
    "artist_gallery.per_page_label": "Par page :",
    "artist_gallery.size_label": "Taille :",
    "artist_gallery.favourites_btn": "Favoris",
    "artist_gallery.favourites_title": "Activer/désactiver le filtre favoris",
    "artist_gallery.manage_btn": "⚙ Gérer",
    "artist_gallery.manage_title": "Gérer les catégories, importer/exporter les favoris",
    "artist_gallery.category_label": "Catégorie :",
    "artist_gallery.category_all": "Tous ({count})",
    "artist_gallery.category_uncat": "Sans catégorie ({count})",
    "artist_gallery.load_error": "Échec du chargement des artistes : {error}",
    "artist_gallery.loading_artists": "chargement des artistes…",
    "artist_gallery.searching": "Recherche…",
    "artist_gallery.search_result_one": "1 résultat pour « {query} »",
    "artist_gallery.search_results": "{count} résultats pour « {query} »",
    "artist_gallery.hint_copy": "Clic droit sur une carte ou survol pour copier son tag.",
    "artist_gallery.prev": "← Préc.",
    "artist_gallery.next": "Suiv. →",
    "artist_gallery.random": "⚄ Aléatoire",
    "artist_gallery.random_title": "Aller à une page aléatoire",
    "artist_gallery.page_of": "Page {page} sur {total}",
    "artist_gallery.artist_count": "{count} artistes",
    "artist_gallery.page_placeholder": "p. #",
    "artist_gallery.go_to_page": "Aller à la page",
    "artist_gallery.no_preview": "pas d'aperçu",
    "artist_gallery.copy_btn": "Copier",
    "artist_gallery.copy_tag_aria": "Copier le tag",
    "artist_gallery.copied": "Copié !",
    "artist_gallery.assign_category_aria": "Assigner une catégorie",
    "artist_gallery.change_category_aria": "Catégorie : {name}. Cliquer pour modifier.",
    "artist_gallery.add_fav_aria": "Ajouter aux favoris",
    "artist_gallery.remove_fav_aria": "Retirer des favoris",
    "artist_gallery.add_fav_title": "Ajouter aux favoris",
    "artist_gallery.remove_fav_title": "Retirer des favoris · clic droit pour catégoriser",
    "artist_gallery.close_cat_picker": "Fermer le sélecteur de catégorie",
    "artist_gallery.cat_uncategorised": "Sans catégorie",
    "artist_gallery.cat_new": "＋ Nouvelle catégorie…",
    "artist_gallery.card_title": "{tag} · Clic droit pour copier le tag",
    "artist_gallery.gen_params.title": "Paramètres de génération d'aperçu",
    "artist_gallery.gen_params.model_stack": "Pile de modèles",
    "artist_gallery.gen_params.sampler_section": "Échantillonneur",
    "artist_gallery.gen_params.positive": "Prompt positif",
    "artist_gallery.gen_params.negative": "Prompt négatif",
    "artist_gallery.gen_params.unet": "UNet",
    "artist_gallery.gen_params.text_encoder": "Encodeur de texte",
    "artist_gallery.gen_params.vae": "VAE",
    "artist_gallery.gen_params.sampler": "Échantillonneur",
    "artist_gallery.gen_params.scheduler": "Planificateur",
    "artist_gallery.gen_params.steps": "Étapes",
    "artist_gallery.gen_params.cfg_scale": "Échelle CFG",
    "artist_gallery.gen_params.seed": "Graine",
    "artist_gallery.gen_params.resolution": "Résolution",
    "artist_gallery.gen_params.output": "Sortie",
    "artist_gallery.lightbox.aria": "Aperçu artiste : {tag}",
    "artist_gallery.lightbox.close_aria": "Fermer",
    "artist_gallery.lightbox.prev_aria": "Artiste précédent",
    "artist_gallery.lightbox.next_aria": "Artiste suivant",
    "artist_gallery.lightbox.no_preview": "aucun aperçu disponible",
    "artist_gallery.lightbox.posts": "{count} posts",
    "artist_gallery.lightbox.aliases": "· alias : {list}",
    "artist_gallery.lightbox.copy_tag": "Copier le tag",
    "artist_gallery.lightbox.insert": "Insérer dans le prompt",
    "artist_gallery.lightbox.close": "Fermer",
    "artist_gallery.fav_manager.aria": "Gérer les catégories de favoris",
    "artist_gallery.fav_manager.close_aria": "Fermer",
    "artist_gallery.fav_manager.title": "Favoris · Catégories et sauvegarde",
    "artist_gallery.fav_manager.categories": "Catégories",
    "artist_gallery.fav_manager.name_label": "Nom",
    "artist_gallery.fav_manager.colour_label": "Couleur",
    "artist_gallery.fav_manager.pick_colour": "Choisir la couleur {colour}",
    "artist_gallery.fav_manager.custom_colour": "Couleur personnalisée",
    "artist_gallery.fav_manager.add_btn": "Ajouter",
    "artist_gallery.fav_manager.name_placeholder": "ex. Portraits",
    "artist_gallery.fav_manager.empty": "Aucune catégorie. Créez-en une ci-dessus pour organiser vos favoris.",
    "artist_gallery.fav_manager.fav_one": "1 fav.",
    "artist_gallery.fav_manager.fav_count": "{count} fav.",
    "artist_gallery.fav_manager.edit_btn": "Modifier",
    "artist_gallery.fav_manager.delete_btn": "Supprimer",
    "artist_gallery.fav_manager.save_btn": "Enregistrer",
    "artist_gallery.fav_manager.cancel_btn": "Annuler",
    "artist_gallery.fav_manager.cat_colour": "Couleur de catégorie",
    "artist_gallery.fav_manager.delete_confirm": "Supprimer la catégorie « {name} » ?",
    "artist_gallery.fav_manager.delete_confirm_used": "Supprimer la catégorie « {name} » ? {count} favori(s) deviendront sans catégorie.",
    "artist_gallery.fav_manager.backup": "Sauvegarde",
    "artist_gallery.fav_manager.backup_desc": "Exporter vos favoris et catégories vers un fichier .json, ou importer un fichier sauvegardé.",
    "artist_gallery.fav_manager.export_btn": "⬇ Exporter les favoris…",
    "artist_gallery.fav_manager.import_mode": "Mode d'import :",
    "artist_gallery.fav_manager.merge": "Fusionner",
    "artist_gallery.fav_manager.replace": "Remplacer",
    "artist_gallery.fav_manager.import_btn": "⬆ Importer les favoris…",
    "artist_gallery.fav_manager.import_result_cat_one": "Importé · {added} ajouté(s), {updated} mis à jour, {categories} nouvelle catégorie.",
    "artist_gallery.fav_manager.import_result_cats": "Importé · {added} ajouté(s), {updated} mis à jour, {categories} nouvelles catégories.",
    "artist_gallery.fav_manager.import_error": "Erreur : {error}",
  },
};

// Load extended locales from companion file if present
const extPath = path.join(__dirname, "artist-gallery-locales-extra.json");
if (fs.existsSync(extPath)) {
  const extra = JSON.parse(fs.readFileSync(extPath, "utf8"));
  for (const [loc, map] of Object.entries(extra)) {
    T[loc] = { ...T[loc], ...map };
  }
}

// German already in de.ts — include for completeness / re-sync
const deMap = parseLocaleKeys("src/lib/locales/de.ts", "artist_gallery.");
T.de = deMap;

const out = {};
for (const loc of ["ja", "fr", "es", "it", "ko", "zh", "zh-tw", "pt", "ru", "de"]) {
  if (!T[loc]) continue;
  out[loc] = {};
  for (const k of keys) {
    if (T[loc][k]) out[loc][k] = T[loc][k];
  }
  const missing = keys.filter((k) => !out[loc][k]);
  if (missing.length) {
    console.warn(`${loc}: missing ${missing.length} keys:`, missing.slice(0, 5).join(", "));
  }
}

const outFile = path.join(__dirname, "i18n-artist-gallery-all.json");
fs.writeFileSync(outFile, JSON.stringify(out, null, 2));
console.log(`Wrote ${outFile}`);
for (const loc of Object.keys(out)) {
  console.log(`  ${loc}: ${Object.keys(out[loc]).length}/${keys.length} keys`);
}
