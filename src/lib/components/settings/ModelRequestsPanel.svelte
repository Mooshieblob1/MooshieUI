<script lang="ts">
  import { modelRequests, type ModelRequest } from "../../stores/modelRequests.svelte.js";
  import { locale } from "../../stores/locale.svelte.js";
  import { onMount } from "svelte";

  let { userRole = "user" }: { userRole?: string } = $props();
  const canManage = $derived(userRole === "admin" || userRole === "moderator");

  let statusFilter = $state<"pending" | "approved" | "denied" | "">("pending");
  let denyModalOpen = $state(false);
  let denyTarget = $state<ModelRequest | null>(null);
  let denyReason = $state("");
  let denyBusy = $state(false);

  onMount(() => {
    if (canManage) {
      modelRequests.fetchRequests("pending");
    }
  });

  async function handleFilterChange() {
    await modelRequests.fetchRequests(statusFilter || undefined);
  }

  async function handleApprove(req: ModelRequest) {
    await modelRequests.approveRequest(req.id);
    await modelRequests.fetchRequests(statusFilter || undefined);
  }

  function openDenyModal(req: ModelRequest) {
    denyTarget = req;
    denyReason = "";
    denyModalOpen = true;
  }

  async function handleDeny() {
    if (!denyTarget) return;
    denyBusy = true;
    await modelRequests.denyRequest(denyTarget.id, denyReason.trim() || undefined);
    denyBusy = false;
    denyModalOpen = false;
    denyTarget = null;
    await modelRequests.fetchRequests(statusFilter || undefined);
  }

  function statusBadge(status: string): string {
    switch (status) {
      case "pending": return "bg-amber-900/50 text-amber-300 border-amber-700/50";
      case "approved": return "bg-emerald-900/50 text-emerald-300 border-emerald-700/50";
      case "denied": return "bg-red-900/50 text-red-300 border-red-700/50";
      default: return "bg-neutral-800 text-neutral-400 border-neutral-700";
    }
  }

  function formatTime(iso: string): string {
    return locale.formatDateTime(iso);
  }
</script>

<div class="space-y-4">
  <div class="flex items-center justify-between">
    <div>
      <h3 class="text-sm font-semibold text-neutral-100">{locale.t("model_requests.title")}</h3>
      <p class="text-xs text-neutral-400 mt-0.5">
        {canManage ? locale.t("model_requests.desc_admin") : locale.t("model_requests.desc_user")}
      </p>
    </div>
    {#if canManage}
      <select
        id="model-request-filter"
        name="modelRequestFilter"
        bind:value={statusFilter}
        onchange={handleFilterChange}
        class="bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-1.5 text-xs text-neutral-200"
      >
        <option value="pending">{locale.t("model_requests.filter_pending")}</option>
        <option value="approved">{locale.t("model_requests.filter_approved")}</option>
        <option value="denied">{locale.t("model_requests.filter_denied")}</option>
        <option value="">{locale.t("model_requests.filter_all")}</option>
      </select>
    {/if}
  </div>

  {#if modelRequests.loading}
    <div class="text-xs text-neutral-500 py-4 text-center">{locale.t("model_requests.loading")}</div>
  {:else if modelRequests.error}
    <div class="text-xs text-red-400 py-4 text-center">{modelRequests.error}</div>
  {:else if modelRequests.requests.length === 0}
    <div class="text-xs text-neutral-500 py-4 text-center">
      {statusFilter === "pending" ? locale.t("model_requests.empty_pending") : locale.t("model_requests.empty")}
    </div>
  {:else}
    <div class="space-y-2">
      {#each modelRequests.requests as req (req.id)}
        <div class="rounded-lg border border-neutral-800 bg-neutral-900/50 p-3 space-y-2">
          <div class="flex items-start justify-between gap-2">
            <div class="min-w-0">
              <p class="text-sm font-medium text-neutral-100 truncate">{req.model_name}</p>
              <p class="text-[11px] text-neutral-400">
                {req.model_type} • {req.file_name}
                {#if req.file_size_kb > 0}
                  • {Math.round(req.file_size_kb / 1024)} MB
                {/if}
              </p>
            </div>
            <span class="shrink-0 px-2 py-0.5 text-[10px] rounded-full border {statusBadge(req.status)}">
              {req.status}
            </span>
          </div>

          <div class="flex items-center gap-3 text-[10px] text-neutral-500">
            <span>{locale.t("model_requests.by", { user: req.username })}</span>
            <span>{formatTime(req.created_at)}</span>
            {#if req.handled_by}
              <span>{locale.t("model_requests.handled_by", { user: req.handled_by })}</span>
            {/if}
          </div>

          {#if req.deny_reason}
            <div class="rounded border border-red-800/50 bg-red-900/20 px-2 py-1.5 text-[11px] text-red-300">
              {locale.t("model_requests.reason", { reason: req.deny_reason })}
            </div>
          {/if}

          {#if canManage && req.status === "pending"}
            <div class="flex items-center gap-2 pt-1">
              <button
                class="px-3 py-1 text-[11px] rounded bg-emerald-600 hover:bg-emerald-500 text-white transition-colors"
                onclick={() => handleApprove(req)}
              >
                {locale.t("model_requests.approve_download")}
              </button>
              <button
                class="px-3 py-1 text-[11px] rounded border border-red-700 text-red-300 hover:bg-red-900/30 transition-colors"
                onclick={() => openDenyModal(req)}
              >
                {locale.t("model_requests.deny")}
              </button>
              <a
                href={req.model_url}
                target="_blank"
                rel="noreferrer"
                class="px-2 py-1 text-[11px] rounded border border-neutral-700 text-neutral-400 hover:text-neutral-200 transition-colors"
              >
                {locale.t("model_requests.view_civitai")}
              </a>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if denyModalOpen && denyTarget}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
    onmousedown={(e) => { if (e.target === e.currentTarget) denyModalOpen = false; }}
    onkeydown={(e) => { if (e.key === "Escape") denyModalOpen = false; }}
  >
    <div class="bg-neutral-900 border border-neutral-700 rounded-xl p-5 w-96 max-w-[92vw] space-y-3">
      <h3 class="text-sm font-semibold text-neutral-100">{locale.t("model_requests.deny_title")}</h3>
      <p class="text-xs text-neutral-400">
        {locale.t("model_requests.deny_body", { model: denyTarget.model_name, user: denyTarget.username })}
      </p>
      <div>
        <label for="deny-reason" class="text-[11px] text-neutral-400 mb-1 block">{locale.t("model_requests.deny_reason_label")}</label>
        <textarea
          id="deny-reason"
          name="denyReason"
          bind:value={denyReason}
          rows="3"
          class="w-full bg-neutral-800 border border-neutral-700 rounded-lg px-3 py-2 text-sm text-neutral-100 placeholder-neutral-500 resize-none"
          placeholder={locale.t("model_requests.deny_reason_placeholder")}
        ></textarea>
      </div>
      <div class="flex items-center justify-end gap-2">
        <button
          class="px-3 py-1.5 text-xs rounded border border-neutral-700 text-neutral-300 hover:text-neutral-100 transition-colors"
          onclick={() => (denyModalOpen = false)}
        >
          {locale.t("common.cancel")}
        </button>
        <button
          class="px-3 py-1.5 text-xs rounded bg-red-600 hover:bg-red-500 text-white transition-colors disabled:opacity-50"
          onclick={handleDeny}
          disabled={denyBusy}
        >
          {denyBusy ? locale.t("model_requests.denying") : locale.t("model_requests.deny_confirm")}
        </button>
      </div>
    </div>
  </div>
{/if}
