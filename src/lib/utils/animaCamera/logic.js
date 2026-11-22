/** Reused from Aitha camera v1.0.1 (MIT); see public/licenses/anima-camera-LICENSE.txt. */
/** Pure prompt logic. Keep in parity with camera_core.py. */
export function createCameraLogic(data) {
  const groupKeys = data.extra_groups.map((group) => group.key);
  const clone = (value) => JSON.parse(JSON.stringify(value));
  const isObject = (value) => value !== null && typeof value === "object" && !Array.isArray(value);
  function number(value, fallback, min = -Infinity, max = Infinity) {
    if (!["number", "string"].includes(typeof value) || (typeof value === "string" && !value.trim())) return fallback;
    if (typeof value === "string" && !/^[+-]?(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:e[+-]?[0-9]+)?$/i.test(value.trim())) return fallback;
    const n = Number(value);
    return Number.isFinite(n) ? Math.max(min, Math.min(max, n)) : fallback;
  }
  function object(value, label) {
    if (value === null || value === undefined || value === "") return {};
    if (typeof value === "string") {
      try { value = JSON.parse(value); }
      catch { throw new Error(`${label} must contain valid JSON.`); }
    }
    if (!isObject(value)) throw new Error(`${label} must be a JSON object.`);
    return value;
  }
  function overlay(defaults, override) {
    if (!isObject(override)) return clone(defaults);
    return Object.fromEntries(Object.entries(defaults).map(([key, value]) => {
      const candidate = override[key] ?? value;
      if (isObject(value)) return [key, overlay(value, candidate)];
      if (typeof value === "number") return [key, number(candidate, value)];
      return [key, typeof value === typeof candidate ? candidate : value];
    }));
  }
  function settings(raw = "{}") {
    const result = overlay(data.defaults, object(raw, "Camera tag settings"));
    result.weight_max = number(result.weight_max, 10, 0.1, 50);
    result.weight_min = number(result.weight_min, 0.1, 0.1, result.weight_max);
    result.extra_master = number(result.extra_master, 1, 0, 20);
    result.xy_mult = number(result.xy_mult, 10, 0.1, 50);
    result.azimuth.weight = number(result.azimuth.weight, 10, 0.1, 50);
    result.azimuth.deadzone_ratio = number(result.azimuth.deadzone_ratio, 0.05, 0, 1);
    for (const axis of ["azimuth", "elevation", "distance", "tilt"]) result[axis].extra = number(result[axis].extra, 0, -50, 50);
    return result;
  }
  function normalizeCamera(horizontal_angle = 0, vertical_angle = 0, zoom = 4, roll = 0) {
    return {
      horizontal_angle: number(horizontal_angle, 0, 0, 360) % 360,
      vertical_angle: number(vertical_angle, 0, -30, 60),
      zoom: number(zoom, 4, 0, 10), roll: number(roll, 0, 0, 1),
    };
  }
  function distanceKey(zoom) {
    if (zoom < 2) return "wide";
    if (zoom < 2.75) return "full";
    if (zoom < 3.5) return "cowboy_shot";
    if (zoom < 6) return "medium";
    if (zoom < 9) return "cu";
    return "ecu";
  }
  const formatWeight = (weight) => (Math.floor(weight * 100 + 0.500000001) / 100).toFixed(2);
  const weightedTags = (value, weight) => value.split(",").map((tag) => tag.trim()).filter(Boolean).map((tag) => `(${tag}:${formatWeight(weight)})`);
  const phrase = (value, weight) => !value.trim() ? "" : weight === 1 ? value.trim() : `(${value.trim()}:${formatWeight(weight)})`;
  const escapeTag = (value) => value.replaceAll("(", "\\(").replaceAll(")", "\\)");
  function buildExtraConfig(values = {}) {
    const label = "extreme_type" in values ? values.extreme_type : "None";
    if (typeof label !== "string") throw new Error("Extreme viewpoint preset must be a string.");
    const extreme = data.extreme_presets.find((p) => p.label === label);
    if (!extreme) throw new Error(`Unknown extreme viewpoint preset: ${label}`);
    const result = { schema_version: 1, extras: {} };
    if (extreme.value) result.extreme = { tag: extreme.kind === "tag" ? escapeTag(extreme.value) : extreme.value, weight: number(values.extreme_weight ?? 1, 1, 0.1, 50) };
    for (const group of data.extra_groups) {
      const key = group.key, selected = `${key}_preset` in values ? values[`${key}_preset`] : "None";
      if (typeof selected !== "string") throw new Error(`${group.label} preset must be a string.`);
      let value = "";
      if (selected === "Custom") value = typeof values[`${key}_custom`] === "string" ? values[`${key}_custom`].trim() : "";
      else if (selected !== "None") {
        const preset = group.presets.find((p) => p.label === selected);
        if (!preset) throw new Error(`Unknown ${group.label} preset: ${selected}`);
        value = preset.kind === "tag" ? escapeTag(preset.value) : preset.value;
      }
      result.extras[key] = { enabled: Boolean(value), value, weight: number(values[`${key}_weight`] ?? 1, 1, 0.1, 10) };
    }
    return result;
  }
  function normalizeExtraConfig(raw = "") {
    const config = object(raw, "Extra configuration");
    if ("schema_version" in config && config.schema_version !== 1) throw new Error("Unsupported extra configuration version; expected 1.");
    const extras = object(config.extras, "Extra configuration extras"), result = { extras: {} };
    for (const key of groupKeys) {
      const entry = extras[key];
      if (isObject(entry) && entry.enabled === true && typeof entry.value === "string" && entry.value.trim()) {
        result.extras[key] = { enabled: true, value: entry.value.trim(), weight: number("weight" in entry ? entry.weight : (key === "dof" ? 1.3 : 1), 1, 0.1, 50) };
      }
    }
    const extremes = [config.extreme, config.extreme_high, config.extreme_low].filter((e) => isObject(e) && typeof e.tag === "string" && e.tag.trim());
    if (extremes.length > 1) throw new Error("Choose one extreme viewpoint; the configuration contains conflicting viewpoints.");
    if (extremes.length) result.extreme = { tag: extremes[0].tag.trim(), weight: number(extremes[0].weight ?? 1, 1, 0.1, 50) };
    return result;
  }
  function extraPrompt(raw = "") {
    const config = normalizeExtraConfig(raw), parts = [];
    if (config.extreme) parts.push(phrase(config.extreme.tag, config.extreme.weight));
    for (const key of groupKeys) if (config.extras[key]) parts.push(phrase(config.extras[key].value, config.extras[key].weight));
    return parts.filter(Boolean).join(", ");
  }
  function cameraPrompt(horizontal = 0, vertical = 0, zoom = 4, roll = 0, tags = "{}", extra = "") {
    const camera = normalizeCamera(horizontal, vertical, zoom, roll), config = settings(tags), extras = normalizeExtraConfig(extra), parts = [];
    const min = config.weight_min, max = config.weight_max, master = config.extra_master;
    const xy = config.xy_mult_enabled ? config.xy_mult : null, maximum = xy !== null ? Math.max(max, xy) : max;
    const azimuth = config.azimuth;
    if (azimuth.enabled && !extras.extreme) {
      const angle = camera.horizontal_angle * Math.PI / 180;
      const ratios = { front: Math.max(0, Math.cos(angle)), back: Math.max(0, -Math.cos(angle)), left: Math.max(0, -Math.sin(angle)), right: Math.max(0, Math.sin(angle)) };
      const total = Object.values(ratios).reduce((a, b) => a + b, 0);
      const budget = (xy !== null ? xy : azimuth.weight) + azimuth.extra * master;
      const cap = maximum + Math.max(0, azimuth.extra * master);
      for (const [direction, ratio] of Object.entries(ratios)) {
        const weight = ratio / total * budget;
        if (ratio > 1e-12 && weight >= azimuth.deadzone_ratio) parts.push(...weightedTags(azimuth.directions[direction].tag, Math.min(cap, Math.max(min, weight))));
      }
    }
    const elevation = config.elevation;
    if (elevation.enabled && !extras.extreme) {
      const angle = camera.vertical_angle, key = angle < -15 ? "low" : angle < 15 ? "eye" : "high";
      const weight = Math.max(1, Math.abs(angle) / 90 * (xy !== null ? xy : 10)) + master * elevation.extra;
      if (weight >= azimuth.deadzone_ratio) parts.push(...weightedTags(elevation.categories[key].tag, Math.min(maximum + Math.max(0, elevation.extra * master), Math.max(min, weight))));
    }
    for (const [axis, tag] of [["distance", config.distance.categories[distanceKey(camera.zoom)].tag], ["tilt", camera.roll > 0 ? config.tilt.dutch_tag : ""]]) {
      if (config[axis].enabled && tag) {
        const extraWeight = config[axis].extra * master;
        parts.push(...weightedTags(tag, Math.min(max + Math.max(0, extraWeight), Math.max(0.1, 1 + extraWeight))));
      }
    }
    const text = extraPrompt(extras);
    if (text) parts.push(text);
    return parts.join(", ");
  }
  function closestPreset(key, value) {
    const options = data.camera_presets[key];
    if (key === "zoom") {
      const keys = ["wide", "full", "cowboy_shot", "medium", "cu", "ecu"];
      return options[keys.indexOf(distanceKey(value))];
    }
    const distance = (candidate) => key === "horizontal_angle" ? Math.min(Math.abs(candidate - value), 360 - Math.abs(candidate - value)) : Math.abs(candidate - value);
    return options.reduce((best, option) => distance(option.value) < distance(best.value) ? option : best);
  }
  return { number, settings, normalizeCamera, distanceKey, buildExtraConfig, normalizeExtraConfig, extraPrompt, cameraPrompt, closestPreset };
}
