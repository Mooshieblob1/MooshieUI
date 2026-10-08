import type { CivitaiModel, CivitaiSort } from "./api.js";

/** Newest publish date across a model's versions, as epoch ms (0 if unknown). */
function latestPublished(model: CivitaiModel): number {
  let latest = 0;
  for (const version of model.modelVersions ?? []) {
    const time = version.publishedAt ? Date.parse(version.publishedAt) : NaN;
    if (!Number.isNaN(time) && time > latest) latest = time;
  }
  return latest;
}

function downloads(model: CivitaiModel): number {
  return model.stats?.downloadCount ?? 0;
}

function thumbsUp(model: CivitaiModel): number {
  return model.stats?.thumbsUpCount ?? 0;
}

/**
 * Orders models by a CivitAI sort option. CivitAI's text search returns
 * relevance order and ignores `sort`, so search results are sorted here.
 * Stable: models that tie keep their relevance order.
 */
export function sortCivitaiModels(models: CivitaiModel[], sort: CivitaiSort): CivitaiModel[] {
  const compare: (a: CivitaiModel, b: CivitaiModel) => number =
    sort === "Newest"
      ? (a, b) => latestPublished(b) - latestPublished(a)
      : sort === "Highest Rated"
        ? (a, b) => thumbsUp(b) - thumbsUp(a) || downloads(b) - downloads(a)
        : (a, b) => downloads(b) - downloads(a);
  return [...models].sort(compare);
}
