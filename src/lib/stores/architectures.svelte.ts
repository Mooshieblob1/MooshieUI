import { listModelArchitectures } from "../utils/api.js";
import { setArchitectures, type ArchitectureSpec } from "../utils/architectures.js";

/**
 * Loads the registered model architectures once and bumps `version`, so
 * components that derive from the registry (Model Hub entries, labels,
 * presets) re-run when it arrives. Leaf store: imports no feature store.
 */
class ArchitecturesStore {
  list = $state<ArchitectureSpec[]>([]);
  version = $state(0);
  private loading: Promise<void> | null = null;

  load(): Promise<void> {
    this.loading ??= listModelArchitectures()
      .then((list) => {
        setArchitectures(list);
        this.list = list;
        this.version += 1;
      })
      .catch((e) => {
        console.warn("Failed to load model architectures:", e);
        this.loading = null;
      });
    return this.loading;
  }
}

export const architectures = new ArchitecturesStore();
