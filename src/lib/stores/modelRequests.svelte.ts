import { isBrowserMode, authHeaders } from "../utils/ipc.js";
import { locale } from "./locale.svelte.js";

export interface ModelRequest {
  id: string;
  username: string;
  model_id: number;
  model_name: string;
  model_type: string;
  model_url: string;
  file_name: string;
  file_url: string;
  file_size_kb: number;
  category: string;
  status: "pending" | "approved" | "denied";
  handled_by?: string;
  deny_reason?: string;
  created_at: string;
  handled_at?: string;
}

export interface AddModelRequestPayload {
  model_id: number;
  model_name: string;
  model_type: string;
  model_url: string;
  file_name: string;
  file_url: string;
  file_size_kb: number;
  category: string;
}

class ModelRequestsStore {
  requests = $state<ModelRequest[]>([]);
  loading = $state(false);
  error = $state<string | null>(null);
  toastMessage = $state<string | null>(null);
  toastKind = $state<"success" | "error" | "info">("info");

  showToast(msg: string, kind: "success" | "error" | "info" = "info") {
    this.toastMessage = msg;
    this.toastKind = kind;
    setTimeout(() => {
      this.toastMessage = null;
    }, 4000);
  }

  async fetchRequests(status?: "pending" | "approved" | "denied") {
    if (!isBrowserMode) {
      this.requests = [];
      return;
    }
    this.loading = true;
    this.error = null;
    try {
      const params = new URLSearchParams();
      if (status) params.set("status", status);
      const url = `/internal-api/_model_requests?${params.toString()}`;
      const resp = await fetch(url, { headers: authHeaders() });
      if (!resp.ok) {
        const data = await resp.json();
        throw new Error(data.error ?? "Failed to fetch requests");
      }
      const data = await resp.json();
      this.requests = data.requests ?? [];
    } catch (e) {
      this.error = String(e);
    } finally {
      this.loading = false;
    }
  }

  async addRequest(payload: AddModelRequestPayload) {
    if (!isBrowserMode) {
      this.showToast(locale.t("model_requests.browser_only"), "error");
      return null;
    }
    try {
      const resp = await fetch("/internal-api/_model_requests/add", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify(payload),
      });
      const data = await resp.json();
      if (!resp.ok) {
        throw new Error(data.error ?? "Failed to submit request");
      }
      this.showToast(locale.t("model_requests.requested", { name: payload.model_name }), "success");
      return data.request as ModelRequest;
    } catch (e) {
      this.showToast(String(e), "error");
      return null;
    }
  }

  async approveRequest(requestId: string) {
    if (!isBrowserMode) return null;
    try {
      const resp = await fetch("/internal-api/_model_requests/approve", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ request_id: requestId }),
      });
      const data = await resp.json();
      if (!resp.ok) {
        throw new Error(data.error ?? "Failed to approve request");
      }
      // Update local state
      const idx = this.requests.findIndex((r) => r.id === requestId);
      if (idx !== -1) this.requests[idx] = data.request;
      this.showToast(locale.t("model_requests.approved"), "success");
      return data.request as ModelRequest;
    } catch (e) {
      this.showToast(String(e), "error");
      return null;
    }
  }

  async denyRequest(requestId: string, reason?: string) {
    if (!isBrowserMode) return null;
    try {
      const resp = await fetch("/internal-api/_model_requests/deny", {
        method: "POST",
        headers: authHeaders({ "Content-Type": "application/json" }),
        body: JSON.stringify({ request_id: requestId, reason: reason || null }),
      });
      const data = await resp.json();
      if (!resp.ok) {
        throw new Error(data.error ?? "Failed to deny request");
      }
      // Update local state
      const idx = this.requests.findIndex((r) => r.id === requestId);
      if (idx !== -1) this.requests[idx] = data.request;
      this.showToast(locale.t("model_requests.denied"), "info");
      return data.request as ModelRequest;
    } catch (e) {
      this.showToast(String(e), "error");
      return null;
    }
  }
}

export const modelRequests = new ModelRequestsStore();