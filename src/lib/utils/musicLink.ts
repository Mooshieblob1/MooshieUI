export interface MusicLinkCapabilities {
  available: boolean;
  downloader: boolean;
  converter: boolean;
  status: "checking" | "downloading" | "verifying" | "retrying" | "error" | "ready";
  tool: string;
  completed: number;
  total: number;
  percent: number | null;
  error: string | null;
}
export interface MusicLinkImport {
  status: "running" | "completed" | "cancelled" | "error";
  error?: string;
  audio_base64?: string;
  title?: string;
  source_url?: string;
  requested_url?: string;
  matched?: boolean;
  duration?: number;
}

/** Only these services need a title/artist match instead of a direct download. */
export function needsSongMatch(raw: string): boolean {
  try { return ["open.spotify.com", "spotify.link", "deezer.com", "www.deezer.com", "link.deezer.com", "deezer.page.link", "tidal.com", "www.tidal.com", "listen.tidal.com"].includes(new URL(raw.trim()).hostname); }
  catch { return false; }
}

export function importedSongFile(result: MusicLinkImport): File {
  if (result.status !== "completed" || !result.audio_base64 || result.audio_base64.length > Math.ceil(64 * 1024 * 1024 / 3) * 4) throw new Error("Invalid imported audio");
  const data = atob(result.audio_base64);
  const bytes = Uint8Array.from(data, character => character.charCodeAt(0));
  const name = (result.title ?? "cover").replace(/[<>:"/\\|?*\x00-\x1f]/g, "_").slice(0, 120);
  return new File([bytes], `${name || "cover"}.mp3`, { type: "audio/mpeg" });
}
