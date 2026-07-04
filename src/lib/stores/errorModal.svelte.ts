import { resolveError } from "../errors/resolveError.js";
import type { FriendlyError } from "../errors/types.js";

class ErrorModalStore {
  current = $state<FriendlyError | null>(null);

  show(raw: unknown) {
    this.current = resolveError(raw);
  }

  close() {
    this.current = null;
  }
}

export const errorModal = new ErrorModalStore();

/** Resolve and display any error in the global modal. */
export function showError(raw: unknown) {
  errorModal.show(raw);
}
